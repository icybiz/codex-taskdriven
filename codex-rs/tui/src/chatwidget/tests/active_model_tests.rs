use super::*;
use codex_app_server_protocol::TurnModelUpdatedNotification;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn notification(
    thread_id: ThreadId,
    turn_id: &str,
    model: &str,
    effort: ReasoningEffortConfig,
) -> ServerNotification {
    ServerNotification::TurnModelUpdated(TurnModelUpdatedNotification {
        sequence: 0,
        thread_id: thread_id.to_string(),
        turn_id: turn_id.to_string(),
        model: model.to_string(),
        effort: Some(effort),
    })
}

fn expected_status_line(model: &str, cwd: &str) -> Option<String> {
    Some(format!("{model} · {}", test_path_display(cwd)))
}

#[tokio::test]
async fn active_model_switch_refreshes_footer_and_announces_both_directions() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(Some("gpt-5.6-sol")).await;
    chat.show_welcome_banner = false;
    chat.config.tui_status_line = Some(vec!["model-with-reasoning".to_string()]);
    chat.set_reasoning_effort(Some(ReasoningEffortConfig::XHigh));
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    chat.turn_lifecycle.last_turn_id = Some("turn-1".to_string());
    chat.on_task_started();
    let _ = drain_insert_history(&mut rx);
    let future_settings = chat.effective_collaboration_mode();

    let astra = notification(
        thread_id,
        "turn-1",
        "gpt-6-astra",
        ReasoningEffortConfig::Max,
    );
    chat.handle_server_notification(astra.clone(), /*replay_kind*/ None);
    // No output delta, manual refresh, keypress, or completed turn is needed.
    assert_eq!(
        status_line_text(&chat),
        expected_status_line("gpt-6-astra max", "/tmp/project")
    );
    assert_eq!(chat.effective_collaboration_mode(), future_settings);
    let history = drain_insert_history(&mut rx);
    insta::assert_snapshot!(
        "active_model_switch_notice",
        lines_to_single_string(&history.concat())
    );

    let width = 90;
    let mut terminal = Terminal::new(TestBackend::new(width, chat.desired_height(width))).unwrap();
    terminal
        .draw(|frame| chat.render(frame.area(), frame.buffer_mut()))
        .unwrap();
    insta::assert_snapshot!(
        "active_model_switch_footer",
        normalized_backend_snapshot(terminal.backend())
    );

    chat.handle_server_notification(astra, /*replay_kind*/ None);
    assert!(
        drain_insert_history(&mut rx).is_empty(),
        "same-model requests must not repeat the notice"
    );
    // A different selection for future submissions must not relabel the
    // already-running Astra request before a new request is captured.
    chat.set_model("gpt-5.4");
    chat.set_reasoning_effort(Some(ReasoningEffortConfig::High));
    assert_eq!(
        status_line_text(&chat),
        expected_status_line("gpt-6-astra max", "/tmp/project")
    );
    chat.handle_server_notification(
        notification(
            thread_id,
            "turn-1",
            "gpt-5.6-sol",
            ReasoningEffortConfig::XHigh,
        ),
        /*replay_kind*/ None,
    );
    assert_eq!(
        status_line_text(&chat),
        expected_status_line("gpt-5.6-sol xhigh", "/tmp/project")
    );
    let history = drain_insert_history(&mut rx);
    assert!(
        lines_to_single_string(&history.concat()).contains("gpt-6-astra max → gpt-5.6-sol xhigh")
    );
}

#[tokio::test]
async fn active_model_ignores_other_threads_and_old_turns() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(Some("gpt-5.6-sol")).await;
    chat.config.tui_status_line = Some(vec!["model-with-reasoning".to_string()]);
    chat.set_reasoning_effort(Some(ReasoningEffortConfig::XHigh));
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    chat.turn_lifecycle.last_turn_id = Some("new-turn".to_string());
    let _ = drain_insert_history(&mut rx);
    for event in [
        notification(
            ThreadId::new(),
            "new-turn",
            "gpt-6-astra",
            ReasoningEffortConfig::Max,
        ),
        notification(
            thread_id,
            "old-turn",
            "gpt-6-astra",
            ReasoningEffortConfig::Max,
        ),
    ] {
        chat.handle_server_notification(event, /*replay_kind*/ None);
    }
    assert_eq!(
        status_line_text(&chat),
        expected_status_line("gpt-5.6-sol xhigh", "/tmp/project")
    );
    assert!(drain_insert_history(&mut rx).is_empty());
}

#[tokio::test]
async fn active_model_next_turn_restores_captured_default_and_reasoning_only_changes() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(Some("gpt-5.6-sol")).await;
    chat.config.tui_status_line = Some(vec!["model-with-reasoning".to_string()]);
    chat.set_reasoning_effort(Some(ReasoningEffortConfig::XHigh));
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    chat.turn_lifecycle.last_turn_id = Some("turn-1".to_string());
    chat.handle_server_notification(
        notification(
            thread_id,
            "turn-1",
            "gpt-6-astra",
            ReasoningEffortConfig::Max,
        ),
        /*replay_kind*/ None,
    );
    chat.turn_lifecycle.last_turn_id = Some("turn-2".to_string());
    chat.handle_server_notification(
        notification(
            thread_id,
            "turn-2",
            "gpt-5.6-sol",
            ReasoningEffortConfig::XHigh,
        ),
        /*replay_kind*/ None,
    );
    assert_eq!(
        status_line_text(&chat),
        expected_status_line("gpt-5.6-sol xhigh", "/tmp/project")
    );
    let _ = drain_insert_history(&mut rx);
    chat.handle_server_notification(
        notification(
            thread_id,
            "turn-2",
            "gpt-5.6-sol",
            ReasoningEffortConfig::High,
        ),
        /*replay_kind*/ None,
    );
    assert_eq!(
        status_line_text(&chat),
        expected_status_line("gpt-5.6-sol high", "/tmp/project")
    );
    assert!(
        lines_to_single_string(&drain_insert_history(&mut rx).concat())
            .contains("gpt-5.6-sol xhigh → gpt-5.6-sol high")
    );
}

#[tokio::test]
async fn active_model_reroute_is_visible_and_manual_idle_selection_can_replace_it() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(Some("gpt-6-astra")).await;
    chat.config.tui_status_line = Some(vec!["model-with-reasoning".to_string()]);
    chat.set_reasoning_effort(Some(ReasoningEffortConfig::Max));
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    chat.turn_lifecycle.last_turn_id = Some("turn-1".to_string());
    let _ = drain_insert_history(&mut rx);
    chat.handle_server_notification(
        ServerNotification::ModelRerouted(codex_app_server_protocol::ModelReroutedNotification {
            sequence: 0,
            thread_id: thread_id.to_string(),
            turn_id: "turn-1".to_string(),
            from_model: "gpt-6-astra".to_string(),
            to_model: "gpt-5.2".to_string(),
            reason: codex_app_server_protocol::ModelRerouteReason::HighRiskCyberActivity,
        }),
        /*replay_kind*/ None,
    );
    assert_eq!(
        status_line_text(&chat),
        expected_status_line("gpt-5.2 max", "/tmp/project")
    );
    assert!(
        lines_to_single_string(&drain_insert_history(&mut rx).concat())
            .contains("gpt-6-astra max → gpt-5.2 max")
    );
    chat.set_model("gpt-5.6-sol");
    chat.set_reasoning_effort(Some(ReasoningEffortConfig::XHigh));
    assert_eq!(
        status_line_text(&chat),
        expected_status_line("gpt-5.6-sol xhigh", "/tmp/project")
    );
}

#[tokio::test]
async fn resumed_active_model_astra_is_correct_on_first_frame_and_ignores_older_events() {
    assert_resumed_active_model(
        "gpt-6-astra",
        ReasoningEffortConfig::Max,
        "gpt-5.6-sol",
        ReasoningEffortConfig::XHigh,
    )
    .await;
}

#[tokio::test]
async fn resumed_active_model_sol_is_correct_on_first_frame_and_ignores_older_events() {
    assert_resumed_active_model(
        "gpt-5.6-sol",
        ReasoningEffortConfig::XHigh,
        "gpt-6-astra",
        ReasoningEffortConfig::Max,
    )
    .await;
}

async fn assert_resumed_active_model(
    actual: &str,
    actual_effort: ReasoningEffortConfig,
    future: &str,
    future_effort: ReasoningEffortConfig,
) {
    let (mut chat, mut rx, _ops) = make_chatwidget_manual(Some(future)).await;
    chat.config.tui_status_line = Some(vec!["model-with-reasoning".to_string()]);
    let thread_id = ThreadId::new();
    let mut session = super::app_server::configured_thread_session(thread_id);
    session.model = future.to_string();
    session.reasoning_effort = Some(future_effort.clone());
    let captured = TurnModelUpdatedNotification {
        thread_id: thread_id.to_string(),
        turn_id: "running-turn".to_string(),
        model: actual.to_string(),
        effort: Some(actual_effort.clone()),
        sequence: 2,
    };
    session.active_model = Some(captured.clone());
    chat.handle_thread_session_quiet(session);
    assert!(chat.turn_lifecycle.agent_turn_running);
    // A subsequent future selection must not clear this live snapshot.
    chat.set_model(future);
    chat.set_reasoning_effort(Some(future_effort.clone()));
    assert_eq!(
        status_line_text(&chat),
        expected_status_line(&format!("{actual} {actual_effort}"), "/tmp/thread-settings",)
    );
    assert_eq!(chat.effective_collaboration_mode().model(), future);
    assert_eq!(
        chat.effective_reasoning_effort(),
        Some(future_effort.clone())
    );
    let _ = drain_insert_history(&mut rx);
    // No model delta or turn-start replay has occurred before this first draw.
    let width = 90;
    let mut terminal = Terminal::new(TestBackend::new(width, chat.desired_height(width))).unwrap();
    terminal
        .draw(|frame| chat.render(frame.area(), frame.buffer_mut()))
        .unwrap();
    insta::assert_snapshot!(
        format!("resumed_active_model_first_frame_{actual}"),
        normalized_backend_snapshot(terminal.backend())
    );
    chat.on_turn_model_updated(TurnModelUpdatedNotification {
        model: future.to_string(),
        effort: Some(future_effort.clone()),
        sequence: 1,
        ..captured.clone()
    });
    assert_eq!(
        status_line_text(&chat),
        expected_status_line(&format!("{actual} {actual_effort}"), "/tmp/thread-settings",)
    );
    assert!(drain_insert_history(&mut rx).is_empty());
    chat.on_turn_model_updated(TurnModelUpdatedNotification {
        model: future.to_string(),
        effort: Some(future_effort.clone()),
        sequence: 3,
        ..captured
    });
    assert_eq!(
        status_line_text(&chat),
        expected_status_line(&format!("{future} {future_effort}"), "/tmp/thread-settings",)
    );
    assert!(
        lines_to_single_string(&drain_insert_history(&mut rx).concat()).contains("Model changed:")
    );
}
