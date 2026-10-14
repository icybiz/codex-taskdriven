//! Presentation of captured inference settings, independent of future submissions.

use super::*;
use codex_app_server_protocol::TurnModelUpdatedNotification;

impl ChatWidget {
    pub(super) fn displayed_reasoning_effort(&self) -> Option<ReasoningEffortConfig> {
        self.turn_lifecycle.active_model.as_ref().map_or_else(
            || self.effective_reasoning_effort(),
            |model| model.effort.clone(),
        )
    }

    pub(super) fn clear_idle_model_display(&mut self) {
        if !self.turn_lifecycle.agent_turn_running {
            self.turn_lifecycle.active_model = None;
        }
    }

    /// Restore a server-owned live snapshot without inventing a new model switch.
    pub(crate) fn restore_active_model_snapshot(&mut self, model: TurnModelUpdatedNotification) {
        if self.thread_id.map(|id| id.to_string()).as_deref() != Some(model.thread_id.as_str()) {
            return;
        }
        self.turn_lifecycle.last_turn_id = Some(model.turn_id.clone());
        self.turn_lifecycle.active_model = Some(model);
        if !self.turn_lifecycle.agent_turn_running {
            self.on_task_started();
        }
        self.refresh_model_dependent_surfaces();
        self.request_redraw();
    }

    pub(super) fn on_turn_model_updated(&mut self, notification: TurnModelUpdatedNotification) {
        if self.thread_id.map(|id| id.to_string()).as_deref()
            != Some(notification.thread_id.as_str())
            || self
                .turn_lifecycle
                .last_turn_id
                .as_deref()
                .is_some_and(|id| id != notification.turn_id)
        {
            return;
        }
        if self
            .turn_lifecycle
            .active_model
            .as_ref()
            .is_some_and(|previous| {
                previous.turn_id == notification.turn_id
                    && previous.sequence > 0
                    && notification.sequence <= previous.sequence
            })
        {
            return;
        }
        let previous_model = self.model_display_name().to_string();
        let previous_effort = self.displayed_reasoning_effort();
        let changed =
            previous_model != notification.model || previous_effort != notification.effort;
        if changed {
            let previous_effort =
                Self::status_line_reasoning_effort_label(previous_effort.as_ref());
            let next_effort =
                Self::status_line_reasoning_effort_label(notification.effort.as_ref());
            self.add_info_message(
                format!(
                    "Model changed: {previous_model} {previous_effort} → {} {next_effort}.",
                    notification.model,
                ),
                /*hint*/ None,
            );
        }
        self.turn_lifecycle.active_model = Some(notification);
        self.refresh_model_dependent_surfaces();
        // Do not wait for an output delta, keypress, or the next user turn.
        self.request_redraw();
    }
}
