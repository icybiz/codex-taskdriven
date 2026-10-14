use super::*;
use crate::context::GuardianNodeReplPolicy;
use crate::context::NodeReplReviewEvidenceMode;
use crate::context::node_repl_review_evidence_mode;
use crate::responses_metadata::CodexResponsesRequestKind;
use crate::turn_metadata::McpTurnMetadataContext;
use codex_protocol::openai_models::ModelInfo;
use pretty_assertions::assert_eq;

fn captured_catalog() -> ModelsResponse {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/taskdriven-models-20260910.json"
    ))
    .expect("unmodified captured catalog")
}

fn captured_context(turn: &Arc<TurnContext>, model: &ModelInfo) -> GuardianReviewContext {
    let mut context = GuardianReviewContext::from(turn);
    context.model_info = Arc::new(model.clone());
    context.reasoning_effort = Some(if model.slug == "gpt-6-astra" {
        ReasoningEffort::Max
    } else {
        ReasoningEffort::XHigh
    });
    context
}

#[tokio::test]
async fn guardian_captured_model_survives_parent_fallback_and_catalog_refresh() {
    let server = start_mock_server().await;
    let (mut session, turn) = guardian_test_session_and_turn(&server).await;
    let catalog = captured_catalog();
    let sol = catalog
        .models
        .iter()
        .find(|model| model.slug == "gpt-5.6-sol")
        .unwrap();
    let astra = catalog
        .models
        .iter()
        .find(|model| model.slug == "gpt-6-astra")
        .unwrap();
    assert!(!sol.node_repl_auto_review_required);
    assert!(astra.node_repl_auto_review_required);
    let old_review = captured_context(&turn, sol);
    let new_review = captured_context(&turn, astra);
    // Neither preferred reviewer nor parent is present after this catalog refresh.
    // Fallback must use the request's captured full metadata, not a later lookup.
    let auth = Arc::clone(&session.services.auth_manager);
    Arc::get_mut(&mut session).unwrap().services.models_manager = Arc::new(
        StaticModelsManager::new(Some(auth), ModelsResponse { models: Vec::new() }),
    );
    let mut policies = Vec::new();
    for context in [&old_review, &new_review, &old_review] {
        let prepared = guardian_review_session_config(&session, context)
            .await
            .unwrap();
        policies.push(prepared.spawn_config.base_instructions.clone());
        assert_eq!(
            prepared.spawn_config.model.as_deref(),
            Some(context.model_info.slug.as_str())
        );
        assert_eq!(
            prepared.spawn_config.model_reasoning_effort,
            Some(ReasoningEffort::Low)
        );
        assert_eq!(
            prepared.node_repl_policy,
            GuardianNodeReplPolicy::from_model_messages(context.model_info.model_messages.as_ref())
        );
        assert_eq!(
            prepared
                .spawn_config
                .features
                .enabled(Feature::RetainClientDeveloperMessages),
            context.model_info.node_repl_auto_review_required,
        );
        assert_eq!(
            node_repl_review_evidence_mode(&turn, &context.model_info),
            if context.model_info.node_repl_auto_review_required {
                NodeReplReviewEvidenceMode::Multimodal
            } else {
                NodeReplReviewEvidenceMode::Disabled
            },
        );
    }
    assert_eq!(policies[0], policies[2]);
    assert!(policies.iter().all(Option::is_some));
    assert_eq!(old_review.model_info.as_ref(), sol);
    assert_eq!(new_review.model_info.as_ref(), astra);
}

#[tokio::test]
async fn guardian_captured_calls_keep_both_node_flags_in_mcp_and_responses_metadata() {
    let server = start_mock_server().await;
    let (session, turn) = guardian_test_session_and_turn(&server).await;
    let catalog = captured_catalog();
    let sol = catalog
        .models
        .iter()
        .find(|model| model.slug == "gpt-5.6-sol")
        .unwrap();
    let astra = catalog
        .models
        .iter()
        .find(|model| model.slug == "gpt-6-astra")
        .unwrap();
    let old = captured_context(&turn, sol);
    let new = captured_context(&turn, astra);
    for context in [&old, &new, &old] {
        let model = &context.model_info;
        let mcp = turn
            .turn_metadata_state
            .current_meta_value_for_mcp_request(McpTurnMetadataContext {
                model: &model.slug,
                reasoning_effort: context.reasoning_effort.clone(),
                node_repl_disabled: model.node_repl_disabled,
                node_repl_auto_review_required: model.node_repl_auto_review_required,
            })
            .unwrap();
        assert_eq!(mcp["model"], model.slug);
        assert_eq!(
            mcp["node_repl_auto_review_required"],
            model.node_repl_auto_review_required
        );
        assert_eq!(mcp["node_repl_disabled"], model.node_repl_disabled);
        let responses = session
            .responses_metadata(&turn, model, CodexResponsesRequestKind::Turn)
            .await;
        assert_eq!(
            responses.node_repl_auto_review_required,
            Some(model.node_repl_auto_review_required)
        );
        assert_eq!(responses.node_repl_disabled, Some(model.node_repl_disabled));
    }
}

#[tokio::test]
async fn guardian_background_model_keeps_current_authority_without_changing_its_origin() {
    let server = start_mock_server().await;
    let (_, turn) = guardian_test_session_and_turn(&server).await;
    let catalog = captured_catalog();
    let sol = catalog
        .models
        .iter()
        .find(|model| model.slug == "gpt-5.6-sol")
        .unwrap();
    let astra = catalog
        .models
        .iter()
        .find(|model| model.slug == "gpt-6-astra")
        .unwrap();
    let mut original = captured_context(&turn, sol);
    original.approvals_reviewer = codex_protocol::config_types::ApprovalsReviewer::User;
    original.approval_policy = AskForApproval::OnRequest;
    let mut current = captured_context(&turn, astra);
    current.approvals_reviewer = codex_protocol::config_types::ApprovalsReviewer::AutoReview;
    current.approval_policy = AskForApproval::Never;
    let restored = current
        .clone()
        .with_model_context(&original.guardian_model_context());
    assert_eq!(restored.model_info, original.model_info);
    assert_eq!(restored.reasoning_effort, original.reasoning_effort);
    assert_eq!(restored.reasoning_summary, original.reasoning_summary);
    assert_eq!(restored.approvals_reviewer, current.approvals_reviewer);
    assert_eq!(restored.approval_policy, current.approval_policy);
    assert_eq!(current.model_info.as_ref(), astra);
}
