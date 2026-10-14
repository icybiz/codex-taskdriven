use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::models::ActivePermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_rollout::RolloutItem;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PersistedResumeSettings {
    pub(super) model_routing: codex_protocol::config_types::ModelRouting,
    pub(super) approval_policy: AskForApproval,
    pub(super) approvals_reviewer: Option<ApprovalsReviewer>,
    pub(super) active_permission_profile: Option<ActivePermissionProfile>,
}

pub(super) fn latest_persisted_resume_settings(
    history: &[RolloutItem],
) -> Option<PersistedResumeSettings> {
    // Routing is a thread selection. A later context snapshot of an older
    // running turn must not resurrect automation after a fixed selection.
    let model_routing = history
        .iter()
        .rev()
        .find_map(|item| match item {
            RolloutItem::EventMsg(EventMsg::ThreadSettingsApplied(event)) => Some(
                event
                    .thread_settings
                    .collaboration_mode
                    .settings
                    .model_routing,
            ),
            _ => None,
        })
        .or_else(|| {
            history.iter().rev().find_map(|item| match item {
                RolloutItem::TurnContext(context) => context
                    .collaboration_mode
                    .as_ref()
                    .map(|mode| mode.settings.model_routing),
                _ => None,
            })
        })
        .unwrap_or_default();
    history
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, item)| match item {
            RolloutItem::TurnContext(turn_context) => Some(PersistedResumeSettings {
                model_routing,
                approval_policy: turn_context.approval_policy,
                approvals_reviewer: turn_context.approvals_reviewer.or_else(|| {
                    history[..index].iter().rev().find_map(|item| match item {
                        RolloutItem::TurnContext(turn_context) => turn_context.approvals_reviewer,
                        RolloutItem::EventMsg(EventMsg::ThreadSettingsApplied(event)) => {
                            Some(event.thread_settings.approvals_reviewer)
                        }
                        _ => None,
                    })
                }),
                active_permission_profile: turn_context.active_permission_profile.clone(),
            }),
            RolloutItem::EventMsg(EventMsg::ThreadSettingsApplied(event)) => {
                Some(PersistedResumeSettings {
                    model_routing,
                    approval_policy: event.thread_settings.approval_policy,
                    approvals_reviewer: Some(event.thread_settings.approvals_reviewer),
                    active_permission_profile: event
                        .thread_settings
                        .active_permission_profile
                        .clone(),
                })
            }
            _ => None,
        })
}

#[cfg(test)]
#[path = "persisted_resume_settings_tests.rs"]
mod tests;
