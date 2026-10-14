use super::*;
use crate::session::TurnInput;
use crate::state::TaskKind;
use crate::tasks::SessionTask;
use crate::tasks::SessionTaskResult;
use pretty_assertions::assert_eq;
use tokio_util::sync::CancellationToken;

struct PendingMcpTask;

impl SessionTask for PendingMcpTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }
    fn span_name(&self) -> &'static str {
        "test.pending_mcp"
    }
    async fn run(
        self: Arc<Self>,
        _session: Arc<Session>,
        _ctx: Arc<TurnContext>,
        _input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> SessionTaskResult {
        cancellation_token.cancelled().await;
        Ok(None)
    }
}

#[tokio::test]
async fn mcp_review_binding_keeps_the_issuing_model_and_checks_server_and_turn() {
    let (session, turn) = make_session_and_context().await;
    let session = Arc::new(session);
    let turn = Arc::new(turn);
    session
        .start_task(Arc::clone(&turn), Vec::new(), PendingMcpTask)
        .await;
    let old_step = StepContext::for_test(Arc::clone(&turn));
    let mut new_step = StepContext::for_test(Arc::clone(&turn));
    let settings = Arc::make_mut(&mut Arc::get_mut(&mut new_step).unwrap().settings);
    Arc::make_mut(&mut settings.model_info).slug = "gpt-6-astra".to_string();
    Arc::make_mut(&mut settings.model_info).node_repl_auto_review_required = true;
    for (call_id, step) in [("old-call", &old_step), ("new-call", &new_step)] {
        session
            .register_mcp_tool_approval_metadata(
                step,
                call_id,
                &McpInvocation {
                    server: "node_repl".to_string(),
                    tool: "js".to_string(),
                    arguments: None,
                },
                approval_metadata(None, None, None, None, None),
            )
            .await;
    }
    for (call_id, expected) in [
        ("old-call", &old_step),
        ("new-call", &new_step),
        ("old-call", &old_step),
    ] {
        let context = session
            .mcp_tool_review_context(&turn.sub_id, call_id, "node_repl")
            .await
            .unwrap();
        assert_eq!(context.model_info, expected.settings.model_info);
        assert_eq!(context.approval_policy, expected.settings.approval_policy());
    }
    for (turn_id, call_id, server) in [
        (turn.sub_id.as_str(), "old-call", "other-server"),
        (turn.sub_id.as_str(), "unknown-call", "node_repl"),
        ("other-turn", "old-call", "node_repl"),
    ] {
        assert!(
            session
                .mcp_tool_review_context(turn_id, call_id, server)
                .await
                .is_none()
        );
    }
    session
        .abort_all_tasks(codex_protocol::protocol::TurnAbortReason::Interrupted)
        .await;
    assert!(
        session
            .mcp_tool_review_context(&turn.sub_id, "old-call", "node_repl")
            .await
            .is_none()
    );
}
