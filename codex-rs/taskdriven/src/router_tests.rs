use super::*;

#[test]
fn scoped_routine_uses_the_efficient_low_route() {
    let decision = resolve_route(
        &builtin_profile(WorkStyle::Standard),
        "writing",
        "auto",
        &RoutingSignals {
            scoped_routine: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(decision.demand, 20);
    assert_eq!(decision.required_role, ModelRole::Efficient);
    assert_eq!(decision.reasoning_effort, ReasoningEffort::Low);
    assert!(decision.reason_codes.contains(&ReasonCode::ScopedRoutine));
}

#[test]
fn work_style_changes_the_frontier_threshold_for_the_same_work() {
    let signals = RoutingSignals {
        visual_verification: true,
        ..Default::default()
    };
    let economy = resolve_route(
        &builtin_profile(WorkStyle::Economy),
        "implementation",
        "web_app",
        &signals,
    )
    .unwrap();
    let quality = resolve_route(
        &builtin_profile(WorkStyle::Quality),
        "implementation",
        "web_app",
        &signals,
    )
    .unwrap();

    assert_eq!(economy.demand, quality.demand);
    assert_eq!(economy.required_role, ModelRole::Balanced);
    assert_eq!(quality.required_role, ModelRole::Frontier);
}

#[test]
fn concrete_execution_failures_escalate_demand_without_reading_logs() {
    let baseline = resolve_route(
        &builtin_profile(WorkStyle::Standard),
        "verification",
        "fix",
        &RoutingSignals::default(),
    )
    .unwrap();
    let failed = resolve_route(
        &builtin_profile(WorkStyle::Standard),
        "verification",
        "fix",
        &RoutingSignals {
            failed_tests: 2,
            scope_expanded: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(baseline.required_role, ModelRole::Balanced);
    assert_eq!(failed.required_role, ModelRole::Frontier);
    assert!(failed.demand > baseline.demand);
    assert!(failed.reason_codes.contains(&ReasonCode::FailedTests));
    assert!(failed.reason_codes.contains(&ReasonCode::ScopeExpansion));
}

#[test]
fn high_risk_work_cannot_be_weakened_by_the_economy_profile() {
    let decision = resolve_route(
        &builtin_profile(WorkStyle::Economy),
        "writing",
        "auto",
        &RoutingSignals {
            high_risk: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(decision.required_role, ModelRole::Frontier);
    assert!(decision.reason_codes.contains(&ReasonCode::HighRisk));
}

#[test]
fn a_profile_ceiling_below_the_safety_floor_is_an_error() {
    let mut profile = builtin_profile(WorkStyle::Economy);
    profile.routing.maximum_role = ModelRole::Balanced;
    let error = resolve_route(
        &profile,
        "writing",
        "auto",
        &RoutingSignals {
            high_risk: true,
            ..Default::default()
        },
    )
    .unwrap_err();

    assert!(matches!(
        error,
        RoutingError::ProfileCeilingBelowRequiredRole {
            required: ModelRole::Frontier,
            maximum: ModelRole::Balanced,
        }
    ));
}

#[test]
fn display_fields_reject_terminal_control_characters() {
    let mut profile = builtin_profile(WorkStyle::Standard);
    profile.name = "Standard\u{1b}[31m".to_string();
    assert!(
        profile
            .validate()
            .unwrap_err()
            .to_string()
            .contains("control characters")
    );
}
