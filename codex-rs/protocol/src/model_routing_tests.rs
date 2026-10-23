use crate::config_types::CollaborationMode;
use crate::config_types::CollaborationModeMask;
use crate::config_types::ModeKind;
use crate::config_types::ModelRouting;
use crate::config_types::Settings;
use crate::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;

#[test]
fn old_settings_stay_fixed_and_explicit_model_choices_exit_taskdriven() {
    let settings: Settings = serde_json::from_value(serde_json::json!({
        "model": "gpt-5.6-sol", "reasoning_effort": "xhigh", "developer_instructions": null
    }))
    .unwrap();
    assert_eq!(settings.model_routing, ModelRouting::Fixed);
    let fixed = CollaborationMode {
        mode: ModeKind::Default,
        settings,
    };
    let mut automatic = fixed.clone();
    automatic.settings.model_routing = ModelRouting::TaskDriven;
    assert_eq!(
        automatic.with_updates(Some("gpt-5.6-sol".to_string()), None, None),
        fixed
    );
    let plan = automatic.apply_mask(&CollaborationModeMask {
        name: "Plan".to_string(),
        mode: Some(ModeKind::Plan),
        model: None,
        reasoning_effort: Some(Some(ReasoningEffort::Medium)),
        developer_instructions: None,
    });
    automatic.mode = ModeKind::Plan;
    assert_eq!(plan, automatic);
}
