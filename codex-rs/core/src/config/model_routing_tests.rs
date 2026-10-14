use super::*;
use codex_protocol::config_types::ModelRouting;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn taskdriven_config_resolves_real_start_model_and_explicit_model_wins() {
    let dir = tempfile::tempdir().unwrap();
    let home = AbsolutePathBuf::try_from(dir.path().to_path_buf()).unwrap();
    let input = ConfigToml {
        model: Some("gpt-6-astra".to_string()),
        model_routing: Some(ModelRouting::TaskDriven),
        model_reasoning_effort: Some(ReasoningEffort::Max),
        ..Default::default()
    };
    for (explicit_model, expected_model, expected_effort, expected_routing) in [
        (
            None,
            "gpt-5.6-sol",
            ReasoningEffort::XHigh,
            ModelRouting::TaskDriven,
        ),
        (
            Some("gpt-6-astra".to_string()),
            "gpt-6-astra",
            ReasoningEffort::Max,
            ModelRouting::Fixed,
        ),
    ] {
        let config = Config::load_from_base_config_with_overrides(
            input.clone(),
            ConfigOverrides {
                model: explicit_model,
                cwd: Some(dir.path().to_path_buf()),
                ..Default::default()
            },
            home.clone(),
        )
        .await
        .unwrap();
        assert_eq!(
            (
                config.model.as_deref(),
                config.model_reasoning_effort,
                config.model_routing
            ),
            (
                Some(expected_model),
                Some(expected_effort),
                expected_routing
            )
        );
    }
}
