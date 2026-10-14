use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_features::Feature;
use codex_models_manager::bundled_models_response;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::openai_models::ToolMode;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::user_input::UserInput;
use codex_taskdriven::TaskDrivenConfig;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn configured_task_switches_model_inside_the_same_turn() -> Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("response-1"),
                responses::ev_function_call(
                    "implement",
                    "set_work_phase",
                    &json!({
                        "phase": "implementation",
                        "reason": "The next operation changes code"
                    })
                    .to_string(),
                ),
                responses::ev_completed("response-1"),
            ]),
            responses::sse_completed("response-2"),
        ],
    )
    .await;
    let home = Arc::new(tempfile::tempdir()?);
    let routing = TaskDrivenConfig {
        enabled: true,
        ..Default::default()
    };
    routing.save(home.path())?;
    let instructions = codex_taskdriven::enable_instructions(None, &routing);
    let test = test_codex()
        .with_home(home)
        .with_model("gpt-5.6-sol")
        .with_config(|config| {
            config
                .features
                .enable(Feature::StepModelSwitching)
                .expect("enable step model switching");
            let template = bundled_models_response()
                .expect("load bundled model catalog")
                .models
                .into_iter()
                .find(|model| model.slug == "gpt-5.5")
                .expect("model template");
            config.model_catalog = Some(ModelsResponse {
                models: ["gpt-5.6-sol", "gpt-6-astra"]
                    .into_iter()
                    .map(|slug| {
                        let mut model = template.clone();
                        model.slug = slug.to_string();
                        model.tool_mode = Some(ToolMode::Direct);
                        model.supported_reasoning_levels = vec![
                            ReasoningEffortPreset {
                                effort: ReasoningEffort::XHigh,
                                description: "Extra high".to_string(),
                            },
                            ReasoningEffortPreset {
                                effort: ReasoningEffort::Max,
                                description: "Maximum".to_string(),
                            },
                        ];
                        model
                    })
                    .collect(),
            });
        })
        .build_with_auto_env(&server)
        .await?;
    test.codex
        .start_or_steer_turn(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Investigate and implement the fix".to_string(),
                text_elements: Vec::new(),
            }])
            .with_thread_settings(ThreadSettingsOverrides {
                collaboration_mode: Some(CollaborationMode {
                    mode: ModeKind::Default,
                    settings: Settings {
                        model: "gpt-5.6-sol".to_string(),
                        reasoning_effort: Some(ReasoningEffort::XHigh),
                        developer_instructions: Some(instructions),
                    },
                }),
                ..Default::default()
            }),
        )
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].body_json()["model"], "gpt-5.6-sol");
    assert_eq!(requests[1].body_json()["model"], "gpt-6-astra");
    assert_eq!(requests[1].body_json()["reasoning"]["effort"], "max");
    assert!(
        requests[0].body_json()["tools"]
            .to_string()
            .contains("set_work_phase")
    );
    assert!(
        requests[0]
            .input()
            .iter()
            .any(|item| item.to_string().contains("<taskdriven_routing>"))
    );
    assert!(
        requests[1]
            .function_call_output_text("implement")
            .is_some_and(|output| output.contains("selected_for_next_model_request"))
    );
    Ok(())
}
