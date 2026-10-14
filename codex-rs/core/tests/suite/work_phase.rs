use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_features::Feature;
use codex_models_manager::bundled_models_response;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::openai_models::ToolMode;
use codex_protocol::protocol::EventMsg;
use codex_protocol::user_input::UserInput;
use codex_taskdriven::TaskDrivenConfig;
use core_test_support::responses;
use core_test_support::test_codex::TestCodexBuilder;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use test_case::test_case;

#[test_case("gpt-5.6-sol", ReasoningEffort::XHigh; "fixed Sol")]
#[test_case("gpt-6-astra", ReasoningEffort::Max; "fixed Astra")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn work_phase_fixed_selection_stays_fixed_with_both_features_enabled(
    model: &str,
    effort: ReasoningEffort,
) -> Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            phase_response("first", "unavailable", "implementation"),
            responses::sse_completed("second"),
        ],
    )
    .await;
    let test = work_phase_test()
        .with_model(model)
        .with_config(move |config| {
            config.model_routing = codex_protocol::config_types::ModelRouting::Fixed;
            config.model_reasoning_effort = Some(effort);
        })
        .build_with_auto_env(&server)
        .await?;
    test.submit_text_turn("Use the explicitly selected model")
        .await?;
    for request in mock.requests() {
        assert_eq!(request.body_json()["model"], model);
        assert!(
            !request.body_json()["tools"]
                .to_string()
                .contains("set_work_phase")
        );
        assert!(
            !request
                .input()
                .iter()
                .any(|item| item.to_string().contains("<autonomous_work_phase>"))
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn work_phase_fixed_selection_stops_automation_in_the_running_turn() -> Result<()> {
    use super::step_settings::answer_paused_turn;
    use super::step_settings::paused_response;
    use super::step_settings::start_paused_turn;
    use codex_protocol::protocol::Op;
    use codex_protocol::protocol::ThreadSettingsOverrides;
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            paused_response("first", "pause"),
            responses::sse_completed("second"),
            responses::sse_completed("third"),
        ],
    )
    .await;
    let test = work_phase_test()
        .with_config(|config| {
            config
                .features
                .enable(Feature::DefaultModeRequestUserInput)
                .unwrap();
        })
        .build_with_auto_env(&server)
        .await?;
    let paused = start_paused_turn(&test.codex).await?;
    test.codex
        .submit(Op::ThreadSettings {
            thread_settings: ThreadSettingsOverrides {
                model: Some("gpt-6-astra".to_string()),
                effort: Some(Some(ReasoningEffort::Max)),
                ..Default::default()
            },
            reply: None,
        })
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::ThreadSettingsApplied(_))
    })
    .await;
    answer_paused_turn(&test.codex, &paused.turn_id).await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    test.submit_text_turn("Continue using the fixed model")
        .await?;
    let requests = mock.requests();
    let models: Vec<_> = requests
        .iter()
        .map(|request| request.body_json()["model"].clone())
        .collect();
    assert_eq!(
        models,
        vec![
            json!("gpt-5.6-sol"),
            json!("gpt-5.6-sol"),
            json!("gpt-6-astra")
        ]
    );
    assert!(
        requests[0].body_json()["tools"]
            .to_string()
            .contains("set_work_phase")
    );
    for request in &requests[1..] {
        assert!(
            !request.body_json()["tools"]
                .to_string()
                .contains("set_work_phase")
        );
        assert!(request.input().iter().any(|item| {
            item.to_string()
                .contains("Autonomous work-phase routing is disabled")
        }));
    }
    assert_eq!(
        test.codex
            .thread_settings_snapshot()
            .await
            .collaboration_mode
            .settings
            .model_routing,
        codex_protocol::config_types::ModelRouting::Fixed
    );
    Ok(())
}

fn work_phase_test() -> TestCodexBuilder {
    let taskdriven = TaskDrivenConfig {
        enabled: true,
        ..Default::default()
    };
    let instructions = codex_taskdriven::enable_instructions(None, &taskdriven);
    test_codex()
        .with_pre_build_hook(move |home| {
            taskdriven
                .save(home)
                .expect("save enabled TaskDriven test configuration");
        })
        .with_model("gpt-5.6-sol")
        .with_config(move |config| {
            config.model_routing = codex_protocol::config_types::ModelRouting::TaskDriven;
            config.developer_instructions = Some(instructions);
            for feature in [Feature::StepModelSwitching, Feature::AutonomousWorkPhase] {
                config
                    .features
                    .enable(feature)
                    .expect("enable work-phase test feature");
            }
            let template = bundled_models_response()
                .expect("load bundled model catalog")
                .models
                .into_iter()
                .find(|model| model.slug == "gpt-5.5")
                .expect("gpt-5.5 template in bundled catalog");
            config.model_catalog = Some(ModelsResponse {
                models: ["gpt-5.6-sol", "gpt-6-astra"]
                    .into_iter()
                    .map(|slug| {
                        let mut model = template.clone();
                        model.slug = slug.to_string();
                        model
                            .model_messages
                            .as_mut()
                            .expect("template model messages")
                            .instructions_template =
                            Some(format!("Instructions for work-phase model {slug}."));
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
            config.model_reasoning_effort = Some(ReasoningEffort::XHigh);
        })
}

fn phase_response(id: &str, call_id: &str, phase: &str) -> String {
    responses::sse(vec![
        responses::ev_response_created(id),
        responses::ev_function_call(
            call_id,
            "set_work_phase",
            &json!({
                "phase": phase, "reason": "The next operation belongs to this phase"
            })
            .to_string(),
        ),
        responses::ev_completed(id),
    ])
}

#[test_case(ToolMode::Direct; "direct tools")]
#[test_case(ToolMode::CodeModeOnly; "code mode tools")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn work_phase_switches_both_ways_in_one_turn_without_losing_history(
    tool_mode: ToolMode,
) -> Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            phase_response("response-1", "implement", "implementation"),
            phase_response("response-2", "verify", "verification"),
            responses::sse(vec![
                responses::ev_assistant_message("answer", "Verified"),
                responses::ev_completed("response-3"),
            ]),
            responses::sse_completed("response-4"),
        ],
    )
    .await;
    let test = work_phase_test()
        .with_config(move |config| {
            for model in &mut config
                .model_catalog
                .as_mut()
                .expect("work-phase test catalog")
                .models
            {
                model.tool_mode = Some(tool_mode);
            }
        })
        .build_with_auto_env(&server)
        .await?;
    let initial_settings = test.codex.thread_settings_snapshot().await;
    let prompt = "Investigate the bug, implement a fix, then run the existing tests.";
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: prompt.to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    let mut displayed_models = Vec::new();
    wait_for_event(&test.codex, |event| match event {
        EventMsg::TurnModelUpdated(model) => {
            displayed_models
                .push(serde_json::to_value(model).expect("serialize model notification"));
            false
        }
        EventMsg::Error(error) => panic!("work-phase test failed: {}", error.message),
        EventMsg::TurnComplete(_) => true,
        _ => false,
    })
    .await;
    assert_eq!(
        test.codex.thread_settings_snapshot().await,
        initial_settings
    );
    test.submit_text_turn("Explain the result").await?;
    let requests = mock.requests();
    let settings: Vec<_> = requests
        .iter()
        .map(|request| {
            let body = request.body_json();
            json!({ "model": body["model"], "effort": body["reasoning"]["effort"] })
        })
        .collect();
    assert_eq!(
        settings,
        vec![
            json!({ "model": "gpt-5.6-sol", "effort": "xhigh" }),
            json!({ "model": "gpt-6-astra", "effort": "max" }),
            json!({ "model": "gpt-5.6-sol", "effort": "xhigh" }),
            json!({ "model": "gpt-5.6-sol", "effort": "xhigh" }),
        ]
    );
    let turn_ids: Vec<String> = requests[..3]
        .iter()
        .map(|request| {
            let metadata: Value = serde_json::from_str(
                &request
                    .header("x-codex-turn-metadata")
                    .expect("turn metadata header"),
            )
            .expect("valid turn metadata JSON");
            metadata["turn_id"]
                .as_str()
                .expect("native turn ID")
                .to_string()
        })
        .collect();
    let expected_display: Vec<_> = settings[..3]
        .iter()
        .enumerate()
        .map(|(index, settings)| {
            let mut event = settings.clone();
            event["sequence"] = json!(index + 1);
            event
        })
        .collect();
    assert_eq!(displayed_models, expected_display);
    assert_eq!(turn_ids, vec![turn_ids[0].clone(); 3]);
    for request in &requests[..3] {
        assert!(
            request
                .input()
                .iter()
                .any(|item| item.to_string().contains(prompt))
        );
        assert!(
            request
                .input()
                .iter()
                .any(|item| item.to_string().contains("<autonomous_work_phase>"))
        );
    }
    let implementation: Value = serde_json::from_str(
        &requests[1]
            .function_call_output_text("implement")
            .expect("implementation phase output"),
    )?;
    let verification: Value = serde_json::from_str(
        &requests[2]
            .function_call_output_text("verify")
            .expect("verification phase output"),
    )?;
    assert_eq!(implementation["thread_id"], verification["thread_id"]);
    assert_eq!(
        implementation["thread_id"],
        test.session_configured.thread_id.to_string()
    );
    assert_eq!(implementation["turn_id"], verification["turn_id"]);
    assert_eq!(implementation["turn_id"], turn_ids[0]);
    assert_eq!(implementation["status"], "selected_for_next_model_request");
    assert_eq!(verification["status"], "selected_for_next_model_request");
    assert!(requests[2].function_call_output_text("implement").is_some());
    assert!(requests[1].input().iter().any(|item| {
        item.to_string()
            .contains("Instructions for work-phase model gpt-6-astra.")
    }));
    assert!(requests[2].input().iter().any(|item| {
        item.to_string()
            .contains("Instructions for work-phase model gpt-5.6-sol.")
    }));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn work_phase_rejects_tools_from_the_old_response_after_switching() -> Result<()> {
    let server = responses::start_mock_server().await;
    let stale_plan = json!({
        "plan": [{"step": "This stale tool must not run", "status": "completed"}]
    });
    let fresh_plan = json!({
        "explanation": "Issued by the new model",
        "plan": [{"step": "Implement the fix", "status": "in_progress"}]
    });
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("response-1"),
                responses::ev_function_call(
                    "implement",
                    "set_work_phase",
                    &json!({"phase": "implementation", "reason": "Implement the fix"}).to_string(),
                ),
                responses::ev_function_call("stale-plan", "update_plan", &stale_plan.to_string()),
                responses::ev_completed("response-1"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("response-2"),
                responses::ev_function_call("fresh-plan", "update_plan", &fresh_plan.to_string()),
                responses::ev_completed("response-2"),
            ]),
            responses::sse_completed("response-3"),
        ],
    )
    .await;
    let test = work_phase_test()
        .with_config(|config| config.update_plan_enabled = true)
        .build_with_auto_env(&server)
        .await?;
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Implement the fix".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    let mut plan_updates = Vec::new();
    wait_for_event(&test.codex, |event| match event {
        EventMsg::PlanUpdate(update) => {
            plan_updates.push(serde_json::to_value(update).unwrap());
            false
        }
        EventMsg::Error(error) => panic!("work-phase test failed: {}", error.message),
        EventMsg::TurnComplete(_) => true,
        _ => false,
    })
    .await;
    let requests = mock.requests();
    assert_eq!(requests.len(), 3);
    assert!(
        requests[0].body_json()["tools"]
            .to_string()
            .contains("update_plan")
    );
    assert_eq!(requests[1].body_json()["model"], "gpt-6-astra");
    let stale_output = requests[1].function_call_output_text("stale-plan").unwrap();
    assert!(
        stale_output.contains("This tool was not executed"),
        "unexpected stale-call result: {stale_output}",
    );
    assert_eq!(requests[2].body_json()["model"], "gpt-6-astra");
    assert_eq!(
        requests[2]
            .function_call_output_text("fresh-plan")
            .as_deref(),
        Some("Plan updated")
    );
    assert_eq!(plan_updates, vec![fresh_plan]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn work_phase_preserves_the_original_model_when_activation_is_rejected() -> Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            phase_response("response-1", "rejected", "implementation"),
            responses::sse_completed("response-2"),
        ],
    )
    .await;
    let test = work_phase_test()
        .with_config(|config| {
            config
                .model_catalog
                .as_mut()
                .unwrap()
                .models
                .iter_mut()
                .find(|model| model.slug == "gpt-6-astra")
                .unwrap()
                .node_repl_disabled = true;
        })
        .build_with_auto_env(&server)
        .await?;
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Implement the fix".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    let mut displayed_models = Vec::new();
    wait_for_event(&test.codex, |event| match event {
        EventMsg::TurnModelUpdated(model) => {
            displayed_models.push(model.clone());
            false
        }
        EventMsg::Error(error) => panic!("work-phase test failed: {}", error.message),
        EventMsg::TurnComplete(_) => true,
        _ => false,
    })
    .await;
    assert_eq!(
        displayed_models,
        (1..=2)
            .map(|sequence| codex_protocol::protocol::TurnModelUpdatedEvent {
                sequence,
                model: "gpt-5.6-sol".to_string(),
                effort: Some(ReasoningEffort::XHigh),
            })
            .collect::<Vec<_>>()
    );
    let requests = mock.requests();
    assert_eq!(requests[1].body_json()["model"], "gpt-5.6-sol");
    assert!(
        requests[1]
            .function_call_output_text("rejected")
            .unwrap()
            .contains("the destination changes the admitted node REPL availability restriction")
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn work_phase_is_opt_in_and_is_unavailable_to_default_sessions() -> Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            phase_response("response-1", "disabled", "implementation"),
            responses::sse_completed("response-2"),
        ],
    )
    .await;
    let test = test_codex().build_with_auto_env(&server).await?;
    test.submit_text_turn("Ordinary request").await?;
    let requests = mock.requests();
    assert!(
        !requests[0].body_json()["tools"]
            .to_string()
            .contains("set_work_phase")
    );
    assert!(
        !requests[0]
            .input()
            .iter()
            .any(|item| item.to_string().contains("<autonomous_work_phase>"))
    );
    assert_eq!(
        requests[0].body_json()["model"],
        requests[1].body_json()["model"]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn work_phase_can_be_enabled_and_retired_when_resuming_an_existing_thread() -> Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse_completed("original-response"),
            phase_response("enabled-response", "implement", "implementation"),
            responses::sse_completed("implementation-response"),
            responses::sse_completed("disabled-response"),
        ],
    )
    .await;
    let original = work_phase_test()
        .with_config(|config| {
            config
                .features
                .disable(Feature::AutonomousWorkPhase)
                .unwrap();
        })
        .build_with_auto_env(&server)
        .await?;
    let retained_context = "Keep this original context across the upgrade.";
    original.submit_text_turn(retained_context).await?;
    let enabled = work_phase_test().restart(&server, &original).await?;
    assert_eq!(
        enabled.session_configured.thread_id,
        original.session_configured.thread_id
    );
    enabled.submit_text_turn("Implement the fix").await?;
    let disabled = work_phase_test()
        .with_config(|config| {
            config
                .features
                .disable(Feature::AutonomousWorkPhase)
                .unwrap();
        })
        .restart(&server, &enabled)
        .await?;
    disabled.submit_text_turn("Explain the result").await?;
    let requests = mock.requests();
    assert_eq!(requests.len(), 4);
    assert!(
        !requests[0].body_json()["tools"]
            .to_string()
            .contains("set_work_phase")
    );
    assert!(
        requests[1].body_json()["tools"]
            .to_string()
            .contains("set_work_phase")
    );
    assert_eq!(requests[2].body_json()["model"], "gpt-6-astra");
    for request in &requests[1..] {
        assert!(
            request
                .input()
                .iter()
                .any(|item| item.to_string().contains(retained_context))
        );
    }
    assert!(requests[1].input().iter().any(|item| {
        item.to_string()
            .contains("Autonomous work-phase routing is enabled")
    }));
    assert!(requests[3].input().iter().any(|item| {
        item.to_string()
            .contains("Autonomous work-phase routing is disabled")
    }));
    assert!(
        !requests[3].body_json()["tools"]
            .to_string()
            .contains("set_work_phase")
    );
    Ok(())
}
