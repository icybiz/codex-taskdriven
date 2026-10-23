//! Live model display snapshots, separate from settings staged for the next request.

use super::session::Session;
use super::turn_context::TurnContext;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::TurnModelUpdatedEvent;

impl Session {
    #[expect(
        clippy::await_holding_invalid_type,
        reason = "active turn identity and model snapshot publication must remain atomic"
    )]
    #[expect(
        clippy::expect_used,
        reason = "request sequence numbers must never wrap or saturate into reused identities"
    )]
    pub(super) async fn record_model_request(
        &self,
        turn: &TurnContext,
        model: String,
        effort: Option<ReasoningEffort>,
    ) -> TurnModelUpdatedEvent {
        let mut snapshot = TurnModelUpdatedEvent {
            model,
            effort,
            sequence: 0,
        };
        let active = self.active_turn.lock().await;
        if let Some(active) = active.as_ref()
            && active.task.as_ref().is_some_and(|task| {
                std::ptr::eq(task.turn_context.as_ref(), turn)
                    && !task.cancellation_token.is_cancelled()
            })
        {
            let mut state = active.turn_state.lock().await;
            snapshot.sequence = state.last_model_request.as_ref().map_or(1, |previous| {
                previous
                    .sequence
                    .checked_add(1)
                    .expect("model sequence overflow")
            });
            state.last_model_request = Some(snapshot.clone());
        }
        snapshot
    }

    #[expect(
        clippy::await_holding_invalid_type,
        reason = "active turn reads must stay consistent with the matching model snapshot"
    )]
    pub(crate) async fn active_turn_model(&self) -> Option<(String, TurnModelUpdatedEvent)> {
        let active = self.active_turn.lock().await;
        let active = active.as_ref()?;
        let task = active.task.as_ref()?;
        if task.cancellation_token.is_cancelled() {
            return None;
        }
        let state = active.turn_state.lock().await;
        Some((
            task.turn_context.sub_id.clone(),
            state.last_model_request.clone()?,
        ))
    }
}
