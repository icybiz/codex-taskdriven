use super::*;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn taskdriven_editor_renders_routes_and_enablement_updates_external_config() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(Some("gpt-5.6-sol")).await;
    chat.set_feature_enabled(Feature::StepModelSwitching, /*enabled*/ true);
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
    chat.set_feature_enabled(Feature::StepModelSwitching, /*enabled*/ true);
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
