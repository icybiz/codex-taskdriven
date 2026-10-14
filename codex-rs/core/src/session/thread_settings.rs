//! Handles persistent thread-settings updates and serializes their persistence
//! with checkpoints written directly to storage.

use super::session::Session;
use super::session::SessionSettingsUpdate;
use super::step_settings::StepSettingsUpdate;
use crate::WithTurnExtensionData;
use crate::config::ConstraintResult;
use codex_history::RolloutItem;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadSettingsAppliedEvent;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::protocol::ThreadSettingsSnapshot;
use codex_thread_store::ThreadStoreResult;
use std::sync::Arc;
use tokio::sync::SemaphorePermit;

impl Session {
    /// Captures and flushes current settings under the shared persistence permit.
    pub(crate) async fn checkpoint_thread_settings(&self) -> ThreadStoreResult<()> {
        let _settings_guard = acquire_persistence_lock(self).await;
        if let Some(live_thread) = self.live_thread() {
            live_thread
                .append_items(&[RolloutItem::EventMsg(applied_event(self).await)])
                .await?;
            live_thread.flush().await?;
        }
        Ok(())
    }
}

/// Applies standalone thread settings. The caller holds the persistence permit through notification.
pub(super) async fn update(
    session: &Session,
    overrides: impl Into<WithTurnExtensionData<ThreadSettingsOverrides>>,
) -> ConstraintResult<ThreadSettingsSnapshot> {
    let updates = prepare_update(overrides);
    let explicit_model_selection =
        updates.step_settings.model.is_some() || updates.step_settings.collaboration_mode.is_some();
    let commit = session.update_settings(updates).await?;
    disable_active_automatic_routing(session, explicit_model_selection, &commit.snapshot).await;
    // Standalone settings changes supersede a pending automatic continuation.
    session.state.lock().await.last_started_turn_id = None;
    Ok(commit.snapshot)
}

/// Converts protocol overrides into the internal settings update shape.
pub(super) fn prepare_update(
    overrides: impl Into<WithTurnExtensionData<ThreadSettingsOverrides>>,
) -> SessionSettingsUpdate {
    let WithTurnExtensionData {
        request: overrides,
        turn_extension_init,
    } = overrides.into();
    let ThreadSettingsOverrides {
        environments,
        runtime_workspace_roots,
        profile_workspace_roots,
        approval_policy,
        approvals_reviewer,
        sandbox_policy,
        permission_profile,
        active_permission_profile,
        windows_sandbox_level,
        model,
        effort,
        summary,
        service_tier,
        collaboration_mode,
        personality,
        disabled_plugin_ids,
    } = overrides;
    SessionSettingsUpdate {
        turn_extension_init,
        step_settings: StepSettingsUpdate {
            model_routing: None,
            model,
            effort,
            collaboration_mode,
            reasoning_summary: summary,
            service_tier,
            personality,
            approval_policy,
            approvals_reviewer,
        },
        environments,
        runtime_workspace_roots,
        profile_workspace_roots,
        sandbox_policy,
        permission_profile,
        active_permission_profile,
        windows_sandbox_level,
        disabled_plugin_ids,
        ..Default::default()
    }
}

/// Acquires the shared permit before capturing or changing persistent settings.
pub(super) async fn acquire_persistence_lock(session: &Session) -> SemaphorePermit<'_> {
    session
        .thread_settings_persistence
        .acquire()
        .await
        .unwrap_or_else(|_| unreachable!("thread settings persistence semaphore is never closed"))
}

/// Applies persistent settings and emits the resulting thread-owned snapshot.
pub(super) async fn apply_update(
    session: &Session,
    submission_id: String,
    updates: SessionSettingsUpdate,
) -> ConstraintResult<()> {
    let _settings_guard = acquire_persistence_lock(session).await;
    let explicit_model_selection =
        updates.step_settings.model.is_some() || updates.step_settings.collaboration_mode.is_some();
    let commit = session.update_settings(updates).await?;
    disable_active_automatic_routing(session, explicit_model_selection, &commit.snapshot).await;
    emit_applied(session, submission_id, commit.snapshot).await;
    Ok(())
}

async fn disable_active_automatic_routing(
    session: &Session,
    explicit_model_selection: bool,
    snapshot: &ThreadSettingsSnapshot,
) {
    if explicit_model_selection
        && snapshot.collaboration_mode.settings.model_routing
            == codex_protocol::config_types::ModelRouting::Fixed
    {
        // A fixed selection disables automatic routing immediately, while the
        // selected model still follows ordinary future-turn semantics. Clone the
        // current snapshot so its model, review authority and request preferences
        // remain exact; already captured requests retain their original snapshot.
        let active = session.active_turn.lock().await;
        if let Some(task) = active.as_ref().and_then(|active| active.task.as_ref()) {
            let current = task.turn_context.next_step_settings.load_full();
            if current.selected.collaboration_mode.settings.model_routing
                == codex_protocol::config_types::ModelRouting::TaskDriven
            {
                let mut fixed = (*current).clone();
                Arc::make_mut(&mut fixed.selected)
                    .collaboration_mode
                    .settings
                    .model_routing = codex_protocol::config_types::ModelRouting::Fixed;
                task.turn_context.next_step_settings.store(Arc::new(fixed));
            }
        }
    }
}

/// Emits the snapshot published by one successful settings update.
pub(super) async fn emit_applied(
    session: &Session,
    submission_id: String,
    snapshot: ThreadSettingsSnapshot,
) {
    let msg = EventMsg::ThreadSettingsApplied(ThreadSettingsAppliedEvent {
        thread_id: Some(session.thread_id()),
        thread_settings: snapshot,
    });
    session
        .send_event_raw_without_materializing_rollout(Event {
            id: submission_id,
            msg,
        })
        .await;
}

/// Builds a current thread-owned snapshot for storage checkpoints.
pub(super) async fn applied_event(session: &Session) -> EventMsg {
    EventMsg::ThreadSettingsApplied(ThreadSettingsAppliedEvent {
        thread_id: Some(session.thread_id()),
        thread_settings: session.thread_settings_snapshot().await,
    })
}
