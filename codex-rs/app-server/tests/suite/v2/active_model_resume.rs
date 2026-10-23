use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::to_response;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnSettingsUpdateResponse;
use codex_app_server_protocol::TurnSettingsUpdateStatus;
use codex_app_server_protocol::TurnStartResponse;
use codex_features::Feature;
use codex_protocol::openai_models::ReasoningEffort;
use core_test_support::responses;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use test_case::test_case;
use tokio::sync::oneshot;
use tokio::time::timeout;

use super::connection_handling_websocket::DEFAULT_READ_TIMEOUT;
use super::connection_handling_websocket::WsClient;
use super::connection_handling_websocket::connect_websocket;
use super::connection_handling_websocket::read_notification_for_method;
use super::connection_handling_websocket::read_response_and_notification_for_method;
use super::connection_handling_websocket::read_response_for_id;
use super::connection_handling_websocket::send_request;
use super::connection_handling_websocket::spawn_websocket_server;

#[test_case(true; "running Astra and future Sol")]
#[test_case(false; "running Sol and staged Astra")]
#[tokio::test]
async fn fresh_connection_resumes_actual_request_before_any_new_model_notification(
    start_with_astra: bool,
) -> Result<()> {
    let (release, gate) = oneshot::channel();
    let (server, _) = start_streaming_sse_server(vec![vec![StreamingSseChunk {
        gate: Some(gate),
        body: responses::sse(vec![
            responses::ev_response_created("held-request"),
            responses::ev_assistant_message("answer", "Done"),
            responses::ev_completed("held-request"),
        ]),
    }]])
    .await;
    let home = TempDir::new()?;
    let catalog = home.path().join("captured-models.json");
    std::fs::write(
        &catalog,
        include_bytes!("../../../../core/tests/fixtures/taskdriven-models-20260910.json"),
    )?;
    MockResponsesConfig::new(server.uri())
        .with_model("gpt-5.6-sol")
        .with_root_config(&format!("model_catalog_json = {}\nmodel_reasoning_effort = \"xhigh\"\nmodel_routing = \"taskDriven\"", serde_json::to_string(&catalog)?))
        .with_provider_config("supports_websockets = false")
        .enable_feature(Feature::StepModelSwitching)
        .enable_feature(Feature::AutonomousWorkPhase)
        .write(home.path())?;
    let (mut process, address) = spawn_websocket_server(home.path()).await?;
    let mut owner = connect_websocket(address).await?;
    initialize(&mut owner).await?;
    send_request(&mut owner, "thread/start", /*id*/ 2, Some(json!({}))).await?;
    let started: ThreadStartResponse =
        to_response(read_response_for_id(&mut owner, /*id*/ 2).await?)?;
    let thread_id = started.thread.id;
    let (actual_model, actual_effort) = if start_with_astra {
        ("gpt-6-astra", ReasoningEffort::Max)
    } else {
        ("gpt-5.6-sol", ReasoningEffort::XHigh)
    };
    send_request(
        &mut owner,
        "turn/start",
        /*id*/ 3,
        Some(json!({
            "threadId": thread_id,
            "input": [{"type": "text", "text": "Explain the current result"}],
            "model": actual_model, "effort": actual_effort,
        })),
    )
    .await?;
    let turn: TurnStartResponse = to_response(read_response_for_id(&mut owner, /*id*/ 3).await?)?;
    timeout(DEFAULT_READ_TIMEOUT, server.wait_for_request_count(1)).await?;
    if start_with_astra {
        // turn/start overrides are persistent; explicitly choose future Sol
        // while the already issued Astra request remains held by the server.
        send_request(
            &mut owner,
            "thread/settings/update",
            /*id*/ 4,
            Some(json!({
                "threadId": thread_id,
                "model": "gpt-5.6-sol", "effort": "xhigh",
            })),
        )
        .await?;
        read_response_and_notification_for_method(
            &mut owner,
            /*id*/ 4,
            "thread/settings/updated",
        )
        .await?;
    } else {
        send_request(
            &mut owner,
            "turn/settings/update",
            /*id*/ 4,
            Some(json!({
                "threadId": thread_id, "turnId": turn.turn.id,
                "model": "gpt-6-astra", "effort": "max",
            })),
        )
        .await?;
        let result: TurnSettingsUpdateResponse =
            to_response(read_response_for_id(&mut owner, /*id*/ 4).await?)?;
        assert_eq!(result.status, TurnSettingsUpdateStatus::Applied);
    }
    owner.close(None).await?;
    drop(owner);
    let mut fresh = connect_websocket(address).await?;
    initialize(&mut fresh).await?;
    send_request(
        &mut fresh,
        "thread/resume",
        /*id*/ 5,
        Some(json!({
            "threadId": thread_id, "excludeTurns": true,
        })),
    )
    .await?;
    let resumed: ThreadResumeResponse =
        to_response(read_response_for_id(&mut fresh, /*id*/ 5).await?)?;
    assert_eq!(resumed.model, "gpt-5.6-sol");
    assert_eq!(resumed.reasoning_effort, Some(ReasoningEffort::XHigh));
    let active = resumed
        .active_model
        .expect("already issued request must be in the resume response");
    assert_eq!(active.turn_id, turn.turn.id);
    assert_eq!(active.model, actual_model);
    assert_eq!(active.effort, Some(actual_effort));
    assert_eq!(active.sequence, 1);
    assert!(resumed.thread.turns.is_empty());
    // No second inference or output delta was needed to reconstruct the display.
    assert_eq!(server.requests().await.len(), 1);
    release
        .send(())
        .expect("held model request should remain pending until release");
    read_notification_for_method(&mut fresh, "turn/completed").await?;
    fresh.close(None).await?;
    process.kill().await?;
    Ok(())
}

async fn initialize(client: &mut WsClient) -> Result<()> {
    send_request(
        client,
        "initialize",
        /*id*/ 1,
        Some(json!({
            "clientInfo": {"name": "active-model-resume-test", "version": "1"},
            "capabilities": {"experimentalApi": true},
        })),
    )
    .await?;
    read_response_for_id(client, /*id*/ 1).await?;
    Ok(())
}
