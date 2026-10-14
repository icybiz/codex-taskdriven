use super::*;
use crate::async_scorer::config::GuardianV2Config;
use codex_protocol::openai_models::ModelsResponse;
use pretty_assertions::assert_eq;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn captured_score_cannot_authorize_another_model_or_a_later_return_to_sol() -> Result<()> {
    let fixture = GuardianFailureFixture::new().await?;
    let store = fixture.test.codex.thread_extension_data();
    let progress = store.get::<GuardianV2ScoreProgress>().unwrap();
    let catalog: ModelsResponse = serde_json::from_str(include_str!(
        "../../../../core/tests/fixtures/taskdriven-models-20260910.json"
    ))?;
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
    let base = store.get::<GuardianV2Config>().unwrap();
    let resolved = |model: &codex_protocol::openai_models::ModelInfo| {
        base.with_model_defaults(
            model
                .model_messages
                .as_ref()
                .and_then(|messages| messages.guardian_v2.as_ref()),
        )
        .unwrap()
    };
    let sol_config = resolved(sol);
    let astra_config = resolved(astra);
    assert_ne!(
        sol_config.classifier_instructions,
        astra_config.classifier_instructions
    );
    let authority = super::super::ScoreAuthorization {
        model: Some(Arc::new(sol.clone())),
        ..super::super::ScoreAuthorization::current(&fixture.test.codex, &Default::default()).await
    };
    let old_generation = progress.begin(sol, &sol_config);
    let score = SecurityRiskScore {
        scores: BTreeMap::from([("action_risk".to_owned(), 0.0)]),
        call_id: Some("old-sol-call".to_string()),
        action: None,
        sampled_at: Some(SystemTime::now().into()),
    };
    assert!(progress.publish_for_generation(
        old_generation,
        score.clone(),
        authority.clone(),
        /*index*/ 1,
    ));
    assert_eq!(
        cached_approval_for_model(&fixture.registry, store, sol).await,
        Some(ReviewDecision::Approved)
    );
    assert_eq!(
        cached_approval_for_model(&fixture.registry, store, astra).await,
        None
    );
    // These completions execute the same publication boundary as an async sample.
    // Hold the old result until two subsequent model configurations have started.
    let (release, complete) = tokio::sync::oneshot::channel();
    let progress_for_completion = Arc::clone(&progress);
    let delayed_score = score.clone();
    let delayed_authority = authority.clone();
    let delayed = tokio::spawn(async move {
        complete.await.unwrap();
        progress_for_completion.publish_for_generation(
            old_generation,
            delayed_score,
            delayed_authority,
            /*index*/ 1,
        )
    });
    progress.begin(astra, &astra_config);
    let new_generation = progress.begin(sol, &sol_config);
    release.send(()).unwrap();
    assert!(!delayed.await?);
    assert_eq!(
        cached_approval_for_model(&fixture.registry, store, sol).await,
        None
    );
    // A fresh result from the returned Sol phase may authorize its own calls.
    let fresh = SecurityRiskScore {
        sampled_at: Some((SystemTime::now() + Duration::from_secs(1)).into()),
        ..score
    };
    assert!(progress.publish_for_generation(
        new_generation,
        fresh,
        authority,
        /*index*/ 2,
    ));
    assert_eq!(
        cached_approval_for_model(&fixture.registry, store, sol).await,
        Some(ReviewDecision::Approved)
    );
    Ok(())
}

async fn cached_approval_for_model(
    registry: &codex_extension_api::ExtensionRegistry<Config>,
    store: &ExtensionData,
    model_info: &codex_protocol::openai_models::ModelInfo,
) -> Option<ReviewDecision> {
    let action = serde_json::Value::Null;
    let permissions = codex_guardian_context::PermissionContext::default();
    let input = codex_extension_api::ApprovalDecisionInput {
        approval_id: "captured-model-cache-probe",
        model_info,
        tool_call_id: None,
        permissions: Some(&permissions),
        action: &action,
        thread_id: codex_protocol::ThreadId::from_string(store.level_id()).unwrap(),
        thread_store: store,
        category: GuardianScope::Shell,
        approval_policy: codex_protocol::protocol::AskForApproval::OnRequest,
        approvals_reviewer: ApprovalsReviewer::AutoReview,
        require_guardian: false,
        require_fresh_review: false,
        full_access: false,
        metrics: None,
        synchronous_reviewer: &CacheMiss,
    };
    match registry.decide_approval(&input).await {
        Some(codex_extension_api::ApprovalDecision::Allow) => Some(ReviewDecision::Approved),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn captured_real_models_reach_the_async_classifier_with_their_own_instructions() -> Result<()>
{
    let catalog: ModelsResponse = serde_json::from_str(include_str!(
        "../../../../core/tests/fixtures/taskdriven-models-20260910.json"
    ))?;
    let mut instructions = Vec::new();
    for slug in ["gpt-5.6-sol", "gpt-6-astra", "gpt-5.6-sol"] {
        let model = catalog
            .models
            .iter()
            .find(|model| model.slug == slug)
            .unwrap()
            .clone();
        // The retained thread model is gpt-5.5. Only the tool's captured metadata
        // can supply the Sol/Astra classifier defaults to this real WS request.
        let (request, _test, _registry) =
            sample_configured_conversation_history_with_source_and_model(
                Vec::new(),
                r#"{"path":"README.md"}"#,
                Some(TEST_GUARDIAN_POLICY),
                "",
                /*model_defaults*/ None,
                ToolCallSource::Direct,
                Some(model),
            )
            .await?;
        instructions.push(
            request["input"][1]["content"][0]["text"]
                .as_str()
                .expect("classifier instructions")
                .to_string(),
        );
    }
    assert_eq!(instructions[0], instructions[2]);
    assert_ne!(instructions[0], instructions[1]);
    Ok(())
}
