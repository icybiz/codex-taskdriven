use crate::ModelRole;
use crate::TaskDrivenConfigError;
use crate::TaskDrivenProfile;
use codex_protocol::openai_models::ReasoningEffort;
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinUseCase {
    Auto,
    Fix,
    Build,
    WebApp,
    DataAutomation,
    InteractiveGame,
    Maintain,
    Research,
}

impl BuiltinUseCase {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "auto" => Some(Self::Auto),
            "fix" => Some(Self::Fix),
            "build" => Some(Self::Build),
            "web_app" => Some(Self::WebApp),
            "data_automation" => Some(Self::DataAutomation),
            "interactive_game" => Some(Self::InteractiveGame),
            "maintain" => Some(Self::Maintain),
            "research" => Some(Self::Research),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoutingSignals {
    pub failed_tests: u8,
    pub scope_expanded: bool,
    pub high_risk: bool,
    pub multi_component: bool,
    pub visual_verification: bool,
    pub command_timeout: bool,
    pub scoped_routine: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    PhaseBaseline,
    UseCaseEmphasis,
    FailedTests,
    ScopeExpansion,
    HighRisk,
    MultiComponent,
    VisualVerification,
    CommandTimeout,
    ScopedRoutine,
    ProfileMinimum,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutingDecision {
    pub demand: u8,
    pub required_role: ModelRole,
    pub reasoning_effort: ReasoningEffort,
    pub reason_codes: Vec<ReasonCode>,
}

#[derive(Debug, Error)]
pub enum RoutingError {
    #[error(transparent)]
    InvalidProfile(#[from] TaskDrivenConfigError),
    #[error("unknown built-in use case {0:?}")]
    UnknownUseCase(String),
    #[error(
        "profile maximum role {maximum:?} is below the required role {required:?} for this work"
    )]
    ProfileCeilingBelowRequiredRole {
        required: ModelRole,
        maximum: ModelRole,
    },
}

pub fn resolve_route(
    profile: &TaskDrivenProfile,
    phase_id: &str,
    use_case_id: &str,
    signals: &RoutingSignals,
) -> Result<RoutingDecision, RoutingError> {
    profile.validate()?;
    let use_case = BuiltinUseCase::from_id(use_case_id)
        .ok_or_else(|| RoutingError::UnknownUseCase(use_case_id.to_string()))?;

    let mut reason_codes = vec![ReasonCode::PhaseBaseline];
    let mut demand = phase_baseline(phase_id);
    let use_case_adjustment = use_case_adjustment(use_case, phase_id);
    if use_case_adjustment > 0 {
        demand = demand.saturating_add(use_case_adjustment);
        reason_codes.push(ReasonCode::UseCaseEmphasis);
    }

    if signals.scoped_routine {
        demand = demand.saturating_sub(10);
        reason_codes.push(ReasonCode::ScopedRoutine);
    }
    if signals.multi_component {
        demand = demand.saturating_add(10);
        reason_codes.push(ReasonCode::MultiComponent);
    }
    if signals.visual_verification {
        demand = demand.saturating_add(5);
        reason_codes.push(ReasonCode::VisualVerification);
    }
    if signals.scope_expanded {
        demand = demand.saturating_add(15);
        reason_codes.push(ReasonCode::ScopeExpansion);
    }
    if signals.command_timeout {
        demand = demand.saturating_add(10);
        reason_codes.push(ReasonCode::CommandTimeout);
    }
    if signals.failed_tests > 0 {
        demand = demand.saturating_add(signals.failed_tests.min(2).saturating_mul(10));
        reason_codes.push(ReasonCode::FailedTests);
    }
    if signals.high_risk {
        demand = demand.max(75);
        reason_codes.push(ReasonCode::HighRisk);
    }
    demand = demand.min(100);

    let (balanced_threshold, frontier_threshold) = thresholds(profile);
    let mut required_role = if demand < balanced_threshold {
        ModelRole::Efficient
    } else if demand < frontier_threshold {
        ModelRole::Balanced
    } else {
        ModelRole::Frontier
    };
    if signals.high_risk {
        required_role = ModelRole::Frontier;
    }
    if role_rank(required_role) < role_rank(profile.routing.minimum_role) {
        required_role = profile.routing.minimum_role;
        reason_codes.push(ReasonCode::ProfileMinimum);
    }
    if role_rank(required_role) > role_rank(profile.routing.maximum_role) {
        return Err(RoutingError::ProfileCeilingBelowRequiredRole {
            required: required_role,
            maximum: profile.routing.maximum_role,
        });
    }

    let preference_bias = i16::from(profile.weights.quality)
        + i16::from(profile.weights.reliability)
        - i16::from(profile.weights.usage)
        - i16::from(profile.weights.latency);
    let effort_demand = (i16::from(demand) + preference_bias / 10).clamp(0, 100) as u8;
    let mut reasoning_effort = if effort_demand < 35 {
        ReasoningEffort::Low
    } else if effort_demand < 70 {
        ReasoningEffort::Medium
    } else if effort_demand < 85 {
        ReasoningEffort::High
    } else {
        ReasoningEffort::XHigh
    };
    if matches!(reasoning_effort, ReasoningEffort::XHigh) && !profile.routing.allow_xhigh {
        reasoning_effort = ReasoningEffort::High;
    }

    Ok(RoutingDecision {
        demand,
        required_role,
        reasoning_effort,
        reason_codes,
    })
}

fn phase_baseline(phase_id: &str) -> u8 {
    match phase_id {
        "writing" => 30,
        "investigation" => 45,
        "implementation" => 55,
        "verification" => 40,
        _ => 45,
    }
}

fn use_case_adjustment(use_case: BuiltinUseCase, phase_id: &str) -> u8 {
    match (use_case, phase_id) {
        (BuiltinUseCase::Auto, _) => 0,
        (BuiltinUseCase::Fix, "investigation") => 10,
        (BuiltinUseCase::Fix, "verification") => 15,
        (BuiltinUseCase::Build, "investigation" | "implementation") => 10,
        (BuiltinUseCase::Build, "verification") => 5,
        (BuiltinUseCase::WebApp, "implementation" | "verification") => 10,
        (BuiltinUseCase::DataAutomation, "implementation") => 5,
        (BuiltinUseCase::DataAutomation, "verification") => 20,
        (BuiltinUseCase::InteractiveGame, "investigation") => 10,
        (BuiltinUseCase::InteractiveGame, "implementation") => 15,
        (BuiltinUseCase::InteractiveGame, "verification") => 10,
        (BuiltinUseCase::Maintain, "investigation") => 10,
        (BuiltinUseCase::Maintain, "verification") => 15,
        (BuiltinUseCase::Research, "investigation") => 20,
        (BuiltinUseCase::Research, "writing") => 15,
        _ => 0,
    }
}

fn thresholds(profile: &TaskDrivenProfile) -> (u8, u8) {
    let quality = i16::from(profile.weights.quality) + i16::from(profile.weights.reliability);
    let efficiency = i16::from(profile.weights.usage) + i16::from(profile.weights.latency);
    let balanced = (40 + efficiency / 5 - quality / 10).clamp(25, 60) as u8;
    let frontier = (75 + efficiency / 4 - quality / 6).clamp(55, 95) as u8;
    (balanced, frontier.max(balanced.saturating_add(10)))
}

fn role_rank(role: ModelRole) -> u8 {
    match role {
        ModelRole::Efficient => 0,
        ModelRole::Balanced => 1,
        ModelRole::Frontier => 2,
    }
}
