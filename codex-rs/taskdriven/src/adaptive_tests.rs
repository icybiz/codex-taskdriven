use super::*;
use codex_protocol::openai_models::ReasoningEffort;
use std::collections::HashSet;
use std::path::Path;
use tempfile::tempdir;

#[test]
fn builtin_profiles_have_distinct_valid_policies() {
    let profiles = builtin_profiles();
    let ids = profiles
        .iter()
        .map(|profile| profile.id.as_str())
        .collect::<HashSet<_>>();

    assert_eq!(profiles.len(), 4);
    assert_eq!(ids.len(), profiles.len());
    for profile in profiles {
        profile.validate().unwrap();
        assert_eq!(profile.license, "Apache-2.0");
        assert!(!profile.routing.allow_max);
    }
}

#[test]
fn adaptive_v2_round_trip_defaults_to_no_confirmation_and_no_paid_speed_tier() {
    let config = TaskDrivenConfigV2::adaptive_default();
    config.validate().unwrap();

    let serialized = toml::to_string_pretty(&config).unwrap();
    let parsed = TaskDrivenConfigV2::parse(Path::new("taskdriven.toml"), &serialized).unwrap();

    assert_eq!(parsed, config);
    let adaptive = parsed.adaptive.unwrap();
    assert_eq!(adaptive.confirmation, SwitchConfirmation::Never);
    assert!(!adaptive.allow_paid_speed_tier);
    assert!(adaptive.pins.is_empty());
}

#[test]
fn v1_document_migrates_to_lossless_fixed_v2_routes() {
    let v1 = TaskDrivenConfig {
        enabled: true,
        initial_task: "implementation".to_string(),
        ..Default::default()
    };
    let v1_text = toml::to_string_pretty(&v1).unwrap();
    let document = TaskDrivenConfigDocument::parse(Path::new("taskdriven.toml"), &v1_text).unwrap();
    assert!(matches!(document, TaskDrivenConfigDocument::V1(_)));

    let v2 = document.into_v2();
    v2.validate().unwrap();
    assert_eq!(v2.mode, TaskDrivenMode::Fixed);
    assert_eq!(v2.initial_phase, v1.initial_task);
    assert!(v2.adaptive.is_none());

    let fixed = v2.fixed.as_ref().unwrap();
    assert_eq!(fixed.routes.len(), v1.tasks.len());
    for task in &v1.tasks {
        let route = fixed.routes.get(&task.id).unwrap();
        assert_eq!(route.model, task.model);
        assert_eq!(route.reasoning_effort, task.reasoning_effort);
    }

    let v2_text = toml::to_string_pretty(&v2).unwrap();
    let reparsed =
        TaskDrivenConfigDocument::parse(Path::new("taskdriven-v2.toml"), &v2_text).unwrap();
    assert_eq!(reparsed, TaskDrivenConfigDocument::V2(v2));
}

#[test]
fn validation_rejects_unsafe_or_ambiguous_profile_shapes() {
    let mut wrong_sum = builtin_profile(WorkStyle::Standard);
    wrong_sum.weights.usage = 19;
    assert!(
        wrong_sum
            .validate()
            .unwrap_err()
            .to_string()
            .contains("add up to 100")
    );

    let mut unsupported_effort = builtin_profile(WorkStyle::Standard);
    unsupported_effort.routing.initial_effort = ReasoningEffort::Ultra;
    assert!(
        unsupported_effort
            .validate()
            .unwrap_err()
            .to_string()
            .contains("initial_effort")
    );

    let mut invalid_range = builtin_profile(WorkStyle::Standard);
    invalid_range.routing.minimum_role = ModelRole::Frontier;
    invalid_range.routing.maximum_role = ModelRole::Efficient;
    assert!(
        invalid_range
            .validate()
            .unwrap_err()
            .to_string()
            .contains("minimum_role")
    );
}

#[test]
fn validation_rejects_mixed_modes_and_unknown_phase_pins() {
    let mut mixed = TaskDrivenConfigV2::adaptive_default();
    mixed.fixed = Some(FixedRouting {
        routes: Default::default(),
    });
    assert!(
        mixed
            .validate()
            .unwrap_err()
            .to_string()
            .contains("must not contain fixed")
    );

    let mut unknown_pin = TaskDrivenConfigV2::adaptive_default();
    unknown_pin.adaptive.as_mut().unwrap().pins.insert(
        "missing".to_string(),
        RoutePin {
            model: "gpt-example".to_string(),
            reasoning_effort: ReasoningEffort::Medium,
        },
    );
    assert!(
        unknown_pin
            .validate()
            .unwrap_err()
            .to_string()
            .contains("does not name")
    );
}

#[test]
fn document_parser_rejects_unknown_versions_before_routing() {
    let error = TaskDrivenConfigDocument::parse(
        Path::new("taskdriven.toml"),
        "version = 99\nenabled = false\n",
    )
    .unwrap_err();
    assert!(error.to_string().contains("unsupported"));
}

#[test]
fn document_load_defaults_to_adaptive_v2_and_saves_atomically() {
    let home = tempdir().unwrap();
    let document = TaskDrivenConfigDocument::load(home.path()).unwrap();
    assert!(matches!(
        document,
        TaskDrivenConfigDocument::V2(TaskDrivenConfigV2 {
            mode: TaskDrivenMode::Adaptive,
            ..
        })
    ));
    assert!(!document.enabled());

    document.save(home.path()).unwrap();
    let loaded = TaskDrivenConfigDocument::load(home.path()).unwrap();
    assert_eq!(loaded, document);
}

#[test]
fn session_activation_does_not_mutate_shared_v1_or_v2_defaults() {
    let models = vec![TaskDrivenModel::new(
        "gpt-6.1-sol",
        ModelRole::Balanced,
        ReasoningEffort::Medium,
        vec![ReasoningEffort::Medium, ReasoningEffort::High],
    )];
    for mut document in [
        TaskDrivenConfigDocument::V1(TaskDrivenConfig::default()),
        TaskDrivenConfigDocument::V2(TaskDrivenConfigV2::adaptive_default()),
    ] {
        for enabled in [false, true] {
            document.set_enabled(enabled);
            let home = tempdir().unwrap();
            document.save(home.path()).unwrap();
            let original = std::fs::read(TaskDrivenConfigDocument::path(home.path())).unwrap();

            let runtime = document.materialize_for_session(&models).unwrap();

            assert!(runtime.enabled);
            assert_eq!(document.enabled(), enabled);
            assert_eq!(document.materialize(&models).unwrap().enabled, enabled);
            assert_eq!(
                std::fs::read(TaskDrivenConfigDocument::path(home.path())).unwrap(),
                original
            );
            assert_eq!(
                TaskDrivenConfigDocument::load(home.path()).unwrap(),
                document
            );
        }
    }
}

#[test]
fn adaptive_document_materializes_against_live_model_candidates() {
    let models = vec![
        TaskDrivenModel::new(
            "gpt-6-luna",
            ModelRole::Efficient,
            ReasoningEffort::Low,
            vec![ReasoningEffort::Low, ReasoningEffort::Medium],
        ),
        TaskDrivenModel::new(
            "gpt-6.1-sol",
            ModelRole::Balanced,
            ReasoningEffort::Medium,
            vec![
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
        ),
        TaskDrivenModel::new(
            "gpt-6-astra",
            ModelRole::Frontier,
            ReasoningEffort::Medium,
            vec![ReasoningEffort::Medium, ReasoningEffort::High],
        ),
    ];
    let document = TaskDrivenConfigDocument::V2(TaskDrivenConfigV2::adaptive_default());
    let runtime = document.materialize(&models).unwrap();

    assert_eq!(runtime.tasks.len(), 4);
    assert_eq!(runtime.task("writing").unwrap().model, "gpt-6-luna");
    assert_eq!(runtime.task("investigation").unwrap().model, "gpt-6.1-sol");
    assert_eq!(runtime.task("implementation").unwrap().model, "gpt-6.1-sol");
}

#[test]
fn adaptive_pin_requires_an_exact_live_model_and_effort() {
    let mut config = TaskDrivenConfigV2::adaptive_default();
    config.adaptive.as_mut().unwrap().pins.insert(
        "implementation".to_string(),
        RoutePin {
            model: "gpt-6-astra".to_string(),
            reasoning_effort: ReasoningEffort::Max,
        },
    );
    let models = vec![TaskDrivenModel::new(
        "gpt-6-astra",
        ModelRole::Frontier,
        ReasoningEffort::Medium,
        vec![ReasoningEffort::Medium],
    )];

    let error = config.materialize(&models).unwrap_err();
    assert!(error.to_string().contains("not supported"));
}

#[test]
fn model_role_classification_accepts_provider_namespaces() {
    assert_eq!(
        classify_model_role("global.openai.gpt-6.1-sol"),
        Some(ModelRole::Balanced)
    );
    assert_eq!(
        classify_model_role("openai.gpt-6-luna"),
        Some(ModelRole::Efficient)
    );
    assert_eq!(classify_model_role("unrelated-model"), None);
}

#[test]
fn explicit_adaptive_migration_preserves_the_exact_v1_file() {
    let home = tempdir().unwrap();
    let v1 = TaskDrivenConfig::default();
    v1.save(home.path()).unwrap();
    let original = std::fs::read(TaskDrivenConfig::path(home.path())).unwrap();

    let migrated = TaskDrivenConfigDocument::migrate_v1_to_adaptive(home.path()).unwrap();
    assert!(matches!(
        migrated,
        TaskDrivenConfigDocument::V2(TaskDrivenConfigV2 {
            mode: TaskDrivenMode::Adaptive,
            ..
        })
    ));
    assert_eq!(
        std::fs::read(home.path().join("taskdriven.toml.v1.bak")).unwrap(),
        original
    );
    assert!(matches!(
        TaskDrivenConfigDocument::load(home.path()).unwrap(),
        TaskDrivenConfigDocument::V2(_)
    ));
}
