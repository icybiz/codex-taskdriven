use super::*;
use codex_protocol::config_types::ModelRouting;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn presets(chat: &ChatWidget) -> Vec<ModelPreset> {
    [
        ("gpt-5.6-sol", ReasoningEffortConfig::XHigh),
        ("gpt-6-astra", ReasoningEffortConfig::Max),
    ]
    .into_iter()
    .map(|(model, effort)| {
        let mut preset = get_available_model(chat, "gpt-5.5");
        preset.id = model.to_string();
        preset.model = model.to_string();
        preset.display_name = model.to_string();
        preset.description = String::new();
        preset.default_reasoning_effort = effort.clone();
        preset.supported_reasoning_efforts = vec![ReasoningEffortPreset {
            effort,
            description: String::new(),
        }];
        preset.is_default = false;
        preset.show_in_picker = true;
        preset
    })
    .collect()
}

fn enable_taskdriven_features(chat: &mut ChatWidget) {
    chat.set_feature_enabled(Feature::AutonomousWorkPhase, /*enabled*/ true);
    chat.set_feature_enabled(Feature::StepModelSwitching, /*enabled*/ true);
}

fn enable_taskdriven(chat: &mut ChatWidget) {
    enable_taskdriven_features(chat);
    codex_taskdriven::TaskDrivenConfig::default()
        .save(chat.config.codex_home.as_path())
        .unwrap();
    let mode = chat.set_taskdriven_enabled(true).unwrap();
    chat.set_effective_collaboration_mode(mode);
}

fn expected_status_line_at(model: &str, cwd: &str) -> Option<String> {
    let model = model
        .replace("gpt-5.6-sol", "GPT-5.6-Sol")
        .replace("gpt-6-astra", "GPT-6-Astra");
    Some(format!("{model} · {}", test_path_display(cwd)))
}

fn expected_status_line(model: &str) -> Option<String> {
    expected_status_line_at(model, "/tmp/project")
}

#[tokio::test]
async fn taskdriven_editor_renders_routes_and_enablement_updates_external_config() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(Some("gpt-5.6-sol")).await;
    enable_taskdriven_features(&mut chat);
    let config = codex_taskdriven::TaskDrivenConfig::default();
    config.save(chat.config.codex_home.as_path()).unwrap();

    chat.open_taskdriven_popup();
    assert_chatwidget_snapshot!(
        "taskdriven_routing_popup",
        render_bottom_popup(&chat, /*width*/ 92)
    );

    let mode = chat.set_taskdriven_enabled(true).unwrap();
    assert_eq!(mode.model(), "gpt-5.6-sol");
    assert_eq!(mode.reasoning_effort(), Some(ReasoningEffort::XHigh));
    assert_eq!(mode.settings.model_routing, ModelRouting::TaskDriven);
    assert!(codex_taskdriven::instructions_are_enabled(
        mode.settings.developer_instructions.as_deref()
    ));
    assert!(
        codex_taskdriven::TaskDrivenConfig::load(chat.config.codex_home.as_path())
            .unwrap()
            .enabled
    );
}

#[tokio::test]
async fn taskdriven_mapping_uses_live_model_and_reasoning_catalog() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(Some("gpt-5.6-sol")).await;
    enable_taskdriven_features(&mut chat);
    codex_taskdriven::TaskDrivenConfig::default()
        .save(chat.config.codex_home.as_path())
        .unwrap();
    let astra = chat
        .model_catalog
        .try_list_models()
        .unwrap()
        .into_iter()
        .find(|preset| preset.model == "gpt-6-astra")
        .expect("Astra in test catalog");

    chat.open_taskdriven_effort_popup("implementation".to_string(), astra);
    assert_chatwidget_snapshot!(
        "taskdriven_reasoning_popup",
        render_bottom_popup(&chat, /*width*/ 72)
    );
}

#[tokio::test]
async fn adaptive_v2_can_be_enabled_and_edited_from_model_settings() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(Some("gpt-6.1-sol")).await;
    enable_taskdriven_features(&mut chat);
    codex_taskdriven::TaskDrivenConfigDocument::V2(
        codex_taskdriven::TaskDrivenConfigV2::adaptive_default(),
    )
    .save(chat.config.codex_home.as_path())
    .unwrap();

    let choices = chat.model_catalog.try_list_models().unwrap();
    let item = chat.taskdriven_selection_item(&choices).unwrap();
    assert_eq!(item.name, "TaskDriven · Standard");
    assert!(
        item.description
            .as_deref()
            .is_some_and(|description| description.contains("Automatic"))
    );

    let mode = chat.set_taskdriven_enabled(true).unwrap();
    assert_eq!(mode.model(), "gpt-6.1-sol");
    assert_eq!(mode.settings.model_routing, ModelRouting::TaskDriven);
    chat.set_effective_collaboration_mode(mode);

    let updated = chat
        .set_taskdriven_profile("builtin:economy@1".to_string())
        .unwrap();
    assert!(updated.is_some());
    chat.set_taskdriven_use_case("web_app".to_string()).unwrap();
    chat.set_taskdriven_mapping(
        "implementation",
        "gpt-6-astra".to_string(),
        ReasoningEffortConfig::High,
    )
    .unwrap();

    let document =
        codex_taskdriven::TaskDrivenConfigDocument::load(chat.config.codex_home.as_path()).unwrap();
    let codex_taskdriven::TaskDrivenConfigDocument::V2(config) = document else {
        panic!("adaptive editor must preserve v2")
    };
    let adaptive = config.adaptive.unwrap();
    assert_eq!(adaptive.profile, "builtin:economy@1");
    assert_eq!(adaptive.use_case, "web_app");
    assert_eq!(
        adaptive.pins.get("implementation"),
        Some(&codex_taskdriven::RoutePin {
            model: "gpt-6-astra".to_string(),
            reasoning_effort: ReasoningEffortConfig::High,
        })
    );
}

#[tokio::test]
async fn v1_editor_migrates_only_on_explicit_adaptive_action_and_keeps_backup() {
    let (chat, _rx, _op_rx) = make_chatwidget_manual(Some("gpt-5.6-sol")).await;
    codex_taskdriven::TaskDrivenConfig::default()
        .save(chat.config.codex_home.as_path())
        .unwrap();
    let original = std::fs::read(codex_taskdriven::TaskDrivenConfig::path(
        chat.config.codex_home.as_path(),
    ))
    .unwrap();

    chat.migrate_taskdriven_to_adaptive().unwrap();

    assert_eq!(
        std::fs::read(chat.config.codex_home.join("taskdriven.toml.v1.bak")).unwrap(),
        original
    );
    assert!(matches!(
        codex_taskdriven::TaskDrivenConfigDocument::load(chat.config.codex_home.as_path()).unwrap(),
        codex_taskdriven::TaskDrivenConfigDocument::V2(_)
    ));
}

#[tokio::test]
async fn taskdriven_is_selected_in_the_picker_and_never_in_the_footer() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(Some("gpt-5.6-sol")).await;
    chat.config.tui_status_line = Some(vec!["model-with-reasoning".to_string()]);
    enable_taskdriven(&mut chat);
    let choices = presets(&chat);
    assert_eq!(chat.model_selection_name(), "taskdriven");
    assert_eq!(
        status_line_text(&chat),
        expected_status_line("gpt-5.6-sol xhigh")
    );
    assert_eq!(
        chat.effective_collaboration_mode().settings.model_routing,
        ModelRouting::TaskDriven
    );
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    chat.turn_lifecycle.last_turn_id = Some("turn-1".to_string());
    chat.on_task_started();
    chat.handle_server_notification(
        ServerNotification::TurnModelUpdated(
            codex_app_server_protocol::TurnModelUpdatedNotification {
                sequence: 0,
                thread_id: thread_id.to_string(),
                turn_id: "turn-1".to_string(),
                model: "gpt-6-astra".to_string(),
                effort: Some(ReasoningEffortConfig::Max),
            },
        ),
        /*replay_kind*/ None,
    );
    assert_eq!(
        status_line_text(&chat),
        expected_status_line("gpt-6-astra max")
    );
    assert_eq!(chat.model_selection_name(), "taskdriven");
    chat.open_model_popup_with_presets(choices);
    let width = 110;
    let mut terminal = Terminal::new(TestBackend::new(width, chat.desired_height(width))).unwrap();
    terminal
        .draw(|frame| chat.render(frame.area(), frame.buffer_mut()))
        .unwrap();
    insta::assert_snapshot!(
        "taskdriven_model_picker",
        normalized_backend_snapshot(terminal.backend())
    );
}

#[tokio::test]
async fn taskdriven_editor_remains_available_to_repair_an_unsupported_route() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(Some("gpt-5.6-sol")).await;
    let choices = presets(&chat);
    assert!(chat.taskdriven_selection_item(&choices).is_none());
    enable_taskdriven_features(&mut chat);
    let item = chat.taskdriven_selection_item(&choices[..1]).unwrap();
    assert!(!item.is_disabled);
    assert!(
        item.description
            .as_deref()
            .is_some_and(|description| description.contains("not in the live catalog"))
    );
    assert!(
        !chat
            .taskdriven_selection_item(&choices)
            .unwrap()
            .is_disabled
    );
}

#[tokio::test]
async fn disabling_taskdriven_returns_to_fixed_model_selection() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(Some("gpt-5.6-sol")).await;
    enable_taskdriven(&mut chat);
    let mode = chat.set_taskdriven_enabled(false).unwrap();
    chat.set_effective_collaboration_mode(mode);
    chat.set_model("gpt-5.6-sol");
    assert_eq!(chat.model_selection_name(), "gpt-5.6-sol");
    assert_eq!(
        chat.effective_collaboration_mode().settings.model_routing,
        ModelRouting::Fixed
    );
}

#[tokio::test]
async fn taskdriven_mode_only_notification_refreshes_idle_footer_and_preserves_active_model() {
    for running in [false, true] {
        let (mut chat, _rx, _op_rx) = make_chatwidget_manual(Some("gpt-5.6-sol")).await;
        chat.config.tui_status_line = Some(vec!["model-with-reasoning".to_string()]);
        enable_taskdriven(&mut chat);
        let thread_id = ThreadId::new();
        chat.thread_id = Some(thread_id);
        chat.turn_lifecycle.last_turn_id = Some("turn-1".to_string());
        if running {
            chat.on_task_started();
        }
        chat.handle_server_notification(
            ServerNotification::TurnModelUpdated(
                codex_app_server_protocol::TurnModelUpdatedNotification {
                    sequence: 0,
                    thread_id: thread_id.to_string(),
                    turn_id: "turn-1".to_string(),
                    model: "gpt-6-astra".to_string(),
                    effort: Some(ReasoningEffortConfig::Max),
                },
            ),
            /*replay_kind*/ None,
        );
        assert_eq!(
            status_line_text(&chat),
            expected_status_line("gpt-6-astra max")
        );

        let mut settings = super::app_server::thread_settings_for_test("gpt-5.6-sol", thread_id);
        settings.thread_settings.effort = Some(ReasoningEffortConfig::XHigh);
        settings.thread_settings.collaboration_mode = chat.effective_collaboration_mode();
        settings
            .thread_settings
            .collaboration_mode
            .settings
            .model_routing = ModelRouting::Fixed;
        chat.handle_server_notification(
            ServerNotification::ThreadSettingsUpdated(settings),
            /*replay_kind*/ None,
        );
        assert_eq!(chat.model_selection_name(), "gpt-5.6-sol");
        assert_eq!(
            status_line_text(&chat),
            expected_status_line_at(
                if running {
                    "gpt-6-astra max"
                } else {
                    "gpt-5.6-sol xhigh"
                },
                "/tmp/thread-settings",
            ),
        );
    }
}

#[tokio::test]
async fn taskdriven_resume_restores_routing_without_replacing_mode_instructions() {
    for routing in [ModelRouting::Fixed, ModelRouting::TaskDriven] {
        let (mut chat, _rx, _op_rx) = make_chatwidget_manual(Some("gpt-5.6-sol")).await;
        enable_taskdriven(&mut chat);
        let instructions = chat
            .effective_collaboration_mode()
            .settings
            .developer_instructions;
        assert!(codex_taskdriven::instructions_are_enabled(
            instructions.as_deref()
        ));
        let mut session = super::app_server::configured_thread_session(ThreadId::new());
        session.model = "gpt-5.6-sol".to_string();
        session.reasoning_effort = Some(ReasoningEffortConfig::XHigh);
        session.model_routing = routing;
        assert_eq!(session.collaboration_mode, None);
        chat.handle_thread_session(session);
        let selected = chat.effective_collaboration_mode();
        assert_eq!(selected.settings.model_routing, routing);
        assert_eq!(selected.settings.developer_instructions, instructions);
    }
}
