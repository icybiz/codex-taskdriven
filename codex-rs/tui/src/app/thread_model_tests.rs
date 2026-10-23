use super::*;
use crate::test_support::test_path_buf;
use codex_app_server_protocol::TurnModelUpdatedNotification;
use codex_protocol::config_types::ModelRouting;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;

#[test]
fn taskdriven_mode_only_selection_clears_idle_model_cache() {
    use codex_protocol::config_types::CollaborationMode;
    use codex_protocol::config_types::ModeKind;
    use codex_protocol::config_types::Settings;

    for collaboration_mode in [
        None,
        Some(CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model: "gpt-5.6-sol".to_string(),
                reasoning_effort: Some(ReasoningEffort::XHigh),
                model_routing: ModelRouting::TaskDriven,
                developer_instructions: None,
            },
        }),
    ] {
        let thread_id = ThreadId::new();
        let mut session = super::tests::test_thread_session(thread_id, test_path_buf("/tmp"));
        session.model = "gpt-5.6-sol".to_string();
        session.reasoning_effort = Some(ReasoningEffort::XHigh);
        session.model_routing = ModelRouting::TaskDriven;
        session.collaboration_mode = collaboration_mode.map(Box::new);
        let mut store =
            ThreadEventStore::new_with_session(/*capacity*/ 8, session.clone(), Vec::new());
        store.push_notification(ServerNotification::TurnModelUpdated(
            TurnModelUpdatedNotification {
                sequence: 0,
                thread_id: thread_id.to_string(),
                turn_id: "turn-1".to_string(),
                model: "gpt-6-astra".to_string(),
                effort: Some(ReasoningEffort::Max),
            },
        ));
        let captured = store.latest_model.clone();
        store.set_session(session.clone(), Vec::new());
        assert_eq!(store.latest_model, captured);

        store.active_turn_id = Some("turn-1".to_string());
        store.clear_idle_model_display_for_settings(
            &session.model,
            session.reasoning_effort.as_ref(),
            ModelRouting::Fixed,
        );
        assert_eq!(
            store.latest_model, captured,
            "disabling automatic selection must not relabel a running request"
        );

        session.model_routing = ModelRouting::Fixed;
        if let Some(mode) = session.collaboration_mode.as_mut() {
            mode.settings.model_routing = ModelRouting::Fixed;
        }
        store.set_session(session, Vec::new());
        assert_eq!(store.latest_model, None);
        assert!(
            store.snapshot().events.is_empty(),
            "a thread switch must not restore old Astra after an idle fixed Sol selection"
        );
    }
}

#[test]
fn active_model_survives_buffer_eviction_and_session_refresh() {
    let thread_id = ThreadId::new();
    let model = TurnModelUpdatedNotification {
        sequence: 0,
        thread_id: thread_id.to_string(),
        turn_id: "turn-1".to_string(),
        model: "gpt-6-astra".to_string(),
        effort: Some(ReasoningEffort::Max),
    };
    // A zero-capacity transcript buffer forces the original event to be evicted.
    let mut store = ThreadEventStore::new(/*capacity*/ 0);
    store.push_notification(ServerNotification::TurnModelUpdated(model.clone()));
    assert!(store.buffer.is_empty());
    // A late event from a prior turn must not replace the retained current model,
    // including after the current turn has completed and has no active ID.
    let mut stale = model.clone();
    stale.turn_id = "old-turn".to_string();
    stale.model = "gpt-5.6-sol".to_string();
    store.push_notification(ServerNotification::TurnModelUpdated(stale));
    store.rebase_buffer_after_session_refresh();
    let snapshot = store.snapshot();
    let restored: Vec<_> = snapshot
        .events
        .into_iter()
        .map(|event| {
            let ThreadBufferedEvent::Notification(notification) = event else {
                panic!("expected captured model notification");
            };
            let ServerNotification::TurnModelUpdated(model) = *notification else {
                panic!("expected captured model notification");
            };
            model
        })
        .collect();
    assert_eq!(restored, vec![model]);
}

#[test]
fn active_model_cache_follows_idle_selection_without_relabeling_running_requests() {
    let thread_id = ThreadId::new();
    let mut session = super::tests::test_thread_session(thread_id, test_path_buf("/tmp"));
    session.model = "gpt-5.6-sol".to_string();
    session.reasoning_effort = Some(ReasoningEffort::XHigh);
    let mut store =
        ThreadEventStore::new_with_session(/*capacity*/ 8, session.clone(), Vec::new());
    store.push_notification(ServerNotification::TurnModelUpdated(
        TurnModelUpdatedNotification {
            sequence: 0,
            thread_id: thread_id.to_string(),
            turn_id: "turn-1".to_string(),
            model: "gpt-6-astra".to_string(),
            effort: Some(ReasoningEffort::Max),
        },
    ));
    let captured = store.latest_model.clone();
    store.set_session(session.clone(), Vec::new());
    assert_eq!(
        store.latest_model, captured,
        "unchanged defaults keep the last actual model"
    );
    store.active_turn_id = Some("turn-1".to_string());
    store.clear_idle_model_display_for_settings(
        "gpt-5.4",
        Some(&ReasoningEffort::High),
        ModelRouting::Fixed,
    );
    assert_eq!(
        store.latest_model, captured,
        "future selection must not relabel a running request"
    );
    session.model = "gpt-5.4".to_string();
    session.reasoning_effort = Some(ReasoningEffort::High);
    store.set_session(session, Vec::new());
    assert!(store.latest_model.is_none());
    assert!(
        store.snapshot().events.is_empty(),
        "old buffered events must not resurrect the prior model"
    );
    // Ordinary backend reroutes can arrive without step-switching notifications.
    // Their replay must retain the known request effort as the live widget does.
    store.push_notification(ServerNotification::ModelRerouted(
        codex_app_server_protocol::ModelReroutedNotification {
            sequence: 0,
            thread_id: thread_id.to_string(),
            turn_id: "turn-2".to_string(),
            from_model: "gpt-5.4".to_string(),
            to_model: "gpt-5.2".to_string(),
            reason: codex_app_server_protocol::ModelRerouteReason::HighRiskCyberActivity,
        },
    ));
    assert_eq!(
        store.latest_model,
        Some(TurnModelUpdatedNotification {
            sequence: 0,
            thread_id: thread_id.to_string(),
            turn_id: "turn-2".to_string(),
            model: "gpt-5.2".to_string(),
            effort: Some(ReasoningEffort::High),
        })
    );
}

#[test]
fn resume_model_snapshot_survives_buffering_and_older_in_flight_notifications() {
    let thread_id = ThreadId::new();
    let captured = TurnModelUpdatedNotification {
        thread_id: thread_id.to_string(),
        turn_id: "running-turn".to_string(),
        model: "gpt-6-astra".to_string(),
        effort: Some(ReasoningEffort::Max),
        sequence: 2,
    };
    let mut session = super::tests::test_thread_session(thread_id, test_path_buf("/tmp"));
    session.model = "gpt-5.6-sol".to_string();
    session.reasoning_effort = Some(ReasoningEffort::XHigh);
    session.active_model = Some(captured.clone());
    let mut store = ThreadEventStore::new_with_session(/*capacity*/ 1, session, Vec::new());
    assert_eq!(store.active_turn_id.as_deref(), Some("running-turn"));
    let mut older = captured.clone();
    older.model = "gpt-5.6-sol".to_string();
    older.sequence = 1;
    store.push_notification(ServerNotification::TurnModelUpdated(older));
    assert_eq!(store.latest_model, Some(captured.clone()));
    assert_eq!(
        store.snapshot().session.unwrap().active_model,
        Some(captured.clone())
    );
    let newer = TurnModelUpdatedNotification {
        model: "gpt-5.6-sol".to_string(),
        effort: Some(ReasoningEffort::XHigh),
        sequence: 3,
        ..captured.clone()
    };
    store.push_notification(ServerNotification::TurnModelUpdated(newer.clone()));
    store.push_notification(ServerNotification::TurnModelUpdated(captured.clone()));
    let mut refreshed = store.snapshot().session.unwrap();
    refreshed.model = "gpt-5.4".to_string();
    refreshed.active_model = Some(captured);
    store.set_session(refreshed, Vec::new());
    assert_eq!(store.snapshot().session.unwrap().active_model, Some(newer));
}
