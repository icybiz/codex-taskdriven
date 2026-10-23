use anyhow::Result;
use codex_features::Feature;
use codex_protocol::config_types::ModelRouting;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;
use codex_taskdriven::TaskDrivenConfig;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn work_phase_real_models_preserve_metadata_and_history_across_both_switches() -> Result<()> {
    let models: ModelsResponse =
        serde_json::from_str(include_str!("../fixtures/taskdriven-models-20260910.json"))?;
    let server = responses::start_mock_server().await;
    let phase = |id, call_id, phase| {
        responses::sse(vec![
            responses::ev_response_created(id),
            responses::ev_function_call(
                call_id,
                "set_work_phase",
                &json!({
                    "phase": phase, "reason": "The next operation requires this work phase"
                })
                .to_string(),
            ),
            responses::ev_completed(id),
        ])
    };
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            phase("r1", "implement", "implementation"),
            phase("r2", "verify", "verification"),
            responses::sse(vec![
                responses::ev_assistant_message("answer", "Verified"),
                responses::ev_completed("r3"),
            ]),
        ],
    )
    .await;
    let captured_catalog = models.clone();
    let taskdriven = TaskDrivenConfig {
        enabled: true,
        ..Default::default()
    };
    let instructions = codex_taskdriven::enable_instructions(None, &taskdriven);
    let test = test_codex()
        .with_pre_build_hook(move |home| {
            taskdriven
                .save(home)
                .expect("save enabled TaskDriven test configuration");
        })
        .with_model("gpt-5.6-sol")
        .with_config(move |config| {
            config.model_catalog = Some(captured_catalog);
            config.model_routing = ModelRouting::TaskDriven;
            config.model_reasoning_effort = Some(ReasoningEffort::XHigh);
            config.developer_instructions = Some(instructions);
            for feature in [
                Feature::StepModelSwitching,
                Feature::AutonomousWorkPhase,
                Feature::GuardianApproval,
                Feature::GuardianV2,
            ] {
                config.features.enable(feature).unwrap();
            }
        })
        .build_with_auto_env(&server)
        .await?;
    let before = test.codex.thread_settings_snapshot().await;
    let prompt = "Investigate this issue, implement the fix, and run the existing tests.";
    test.submit_text_turn(prompt).await?;
    assert_eq!(test.config.model_catalog, Some(models));
    assert_eq!(test.codex.thread_settings_snapshot().await, before);
    let requests = mock.requests();
    assert_eq!(requests.len(), 3);
    let mut turn_ids = Vec::new();
    for (request, (model, effort, required)) in requests.iter().zip([
        ("gpt-5.6-sol", "xhigh", false),
        ("gpt-6-astra", "max", true),
        ("gpt-5.6-sol", "xhigh", false),
    ]) {
        let body = request.body_json();
        assert_eq!(body["model"], model);
        assert_eq!(body["reasoning"]["effort"], effort);
        let metadata: Value =
            serde_json::from_str(&request.header("x-codex-turn-metadata").expect("metadata"))?;
        assert_eq!(metadata["node_repl_auto_review_required"], required);
        assert_eq!(metadata["node_repl_disabled"], false);
        turn_ids.push(metadata["turn_id"].clone());
        assert!(
            request
                .input()
                .iter()
                .any(|item| item.to_string().contains(prompt))
        );
    }
    assert_eq!(turn_ids, vec![turn_ids[0].clone(); 3]);
    assert!(requests[1].function_call_output_text("implement").is_some());
    assert!(requests[2].function_call_output_text("verify").is_some());
    Ok(())
}
