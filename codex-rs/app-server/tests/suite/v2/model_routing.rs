use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadForkResponse;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_app_server_protocol::ThreadSettingsUpdateResponse;
use codex_app_server_protocol::ThreadSettingsUpdatedNotification;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::ModelRouting;
use codex_protocol::config_types::Settings;
use codex_protocol::openai_models::ReasoningEffort;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use test_case::test_case;

#[test_case(ModelRouting::TaskDriven, "fixed", ThreadHistoryMode::Legacy; "legacy automatic")]
#[test_case(ModelRouting::Fixed, "taskDriven", ThreadHistoryMode::Legacy; "legacy fixed")]
#[test_case(ModelRouting::TaskDriven, "fixed", ThreadHistoryMode::Paginated; "paginated automatic")]
#[test_case(ModelRouting::Fixed, "taskDriven", ThreadHistoryMode::Paginated; "paginated fixed")]
#[tokio::test]
async fn model_routing_survives_warm_cold_resume_and_fork(
    routing: ModelRouting,
    later_default: &str,
    history_mode: ThreadHistoryMode,
) -> Result<()> {
    let server = responses::start_mock_server().await;
    let requests =
        responses::mount_sse_once(&server, responses::sse_completed("materialized")).await;
    let home = TempDir::new()?;
    let config = || {
        MockResponsesConfig::new(&server.uri())
            .with_model("gpt-5.6-sol")
            .enable_feature(Feature::StepModelSwitching)
            .enable_feature(Feature::AutonomousWorkPhase)
    };
    config().write(home.path())?;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let id = app
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            history_mode: Some(history_mode),
            ..Default::default()
        })
        .await?;
    let started: ThreadStartResponse = app.read_response(id).await?;
    let thread_id = started.thread.id;
    let id = app
        .send_turn_start_request(TurnStartParams {
            thread_id: thread_id.clone(),
            input: vec![UserInput::Text {
                text: "Keep this conversation".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let turn: TurnStartResponse = app.read_response(id).await?;
    let completed: TurnCompletedNotification = app.read_notification("turn/completed").await?;
    assert_eq!(completed.turn.id, turn.turn.id);
    let id = app
        .send_thread_settings_update_request(ThreadSettingsUpdateParams {
            thread_id: thread_id.clone(),
            collaboration_mode: Some(CollaborationMode {
                mode: ModeKind::Default,
                settings: Settings {
                    model: "gpt-5.6-sol".to_string(),
                    reasoning_effort: Some(ReasoningEffort::XHigh),
                    model_routing: routing,
                    developer_instructions: None,
                },
            }),
            ..Default::default()
        })
        .await?;
    let _: ThreadSettingsUpdateResponse = app.read_response(id).await?;
    let updated: ThreadSettingsUpdatedNotification =
        app.read_notification("thread/settings/updated").await?;
    assert_eq!(
        updated
            .thread_settings
            .collaboration_mode
            .settings
            .model_routing,
        routing
    );
    for cold in [false, true] {
        if cold {
            app.shutdown_gracefully().await?;
            config()
                .with_root_config(&format!("model_routing = \"{later_default}\""))
                .write(home.path())?;
            app = TestAppServer::builder()
                .with_codex_home(home.path())
                .build_initialized()
                .await?;
        }
        let id = app
            .send_thread_resume_request(ThreadResumeParams {
                thread_id: thread_id.clone(),
                ..Default::default()
            })
            .await?;
        let resumed: ThreadResumeResponse = app.read_response(id).await?;
        assert_eq!(
            (resumed.thread.id, resumed.model_routing, resumed.model),
            (thread_id.clone(), routing, "gpt-5.6-sol".to_string()),
        );
        assert_eq!(resumed.thread.turns.len(), 1);
        assert_eq!(resumed.thread.turns[0].id, turn.turn.id);
        assert!(resumed.thread.turns[0].items.iter().any(|item| {
            matches!(item, codex_app_server_protocol::ThreadItem::UserMessage { content, .. }
                if content.iter().any(|input| matches!(input,
                    UserInput::Text { text, .. } if text == "Keep this conversation")))
        }));
        let id = app
            .send_thread_fork_request(ThreadForkParams {
                thread_id: thread_id.clone(),
                ..Default::default()
            })
            .await?;
        let forked: ThreadForkResponse = app.read_response(id).await?;
        assert_eq!(forked.model_routing, routing);
        assert_eq!(forked.thread.turns.len(), 1);
    }
    // An explicit fixed model overrides a saved automatic policy on cold resume.
    app.shutdown_gracefully().await?;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let id = app
        .send_thread_resume_request(ThreadResumeParams {
            thread_id,
            model: Some("gpt-6-astra".to_string()),
            ..Default::default()
        })
        .await?;
    let resumed: ThreadResumeResponse = app.read_response(id).await?;
    assert_eq!(
        (resumed.model_routing, resumed.model),
        (ModelRouting::Fixed, "gpt-6-astra".to_string()),
    );
    app.shutdown_gracefully().await?;
    assert_eq!(
        requests.single_request().body_json()["model"],
        "gpt-5.6-sol"
    );
    Ok(())
}
