use super::*;
use codex_app_server_protocol::TurnModelUpdatedNotification;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn resume_active_model_survives_history_and_input_replay_until_current_completion() {
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    let thread_id = ThreadId::new();
    let mut session = test_thread_session(thread_id, test_path_buf("/tmp/project"));
    session.model = "gpt-5.6-sol".to_string();
    session.reasoning_effort = Some(ReasoningEffort::XHigh);
    app.chat_widget.handle_thread_session(session.clone());
    app.chat_widget
        .apply_external_edit("retained draft".to_string());
    let input_state = app.chat_widget.capture_thread_input_state();
    session.active_model = Some(TurnModelUpdatedNotification {
        thread_id: thread_id.to_string(),
        turn_id: "running-turn".to_string(),
        model: "gpt-6-astra".to_string(),
        effort: Some(ReasoningEffort::Max),
        sequence: 2,
    });
    for completed in [false, true] {
        let events = if completed {
            vec![ThreadBufferedEvent::Notification(Box::new(
                turn_completed_notification(thread_id, "running-turn", TurnStatus::Completed),
            ))]
        } else {
            Vec::new()
        };
        app.replay_thread_snapshot(
            ThreadEventSnapshot {
                session: Some(session.clone()),
                delegated_turns: Vec::new(),
                // Historical completion and idle input state must not clear the
                // independently captured current request on the first frame.
                turns: vec![test_turn("old-turn", TurnStatus::Completed, Vec::new())],
                events,
                active_reasoning_item: None,
                input_state: input_state.clone(),
            },
            /*resume_restored_queue*/ false,
        );
        assert_eq!(app.chat_widget.is_task_running_for_test(), !completed);
    }
}
