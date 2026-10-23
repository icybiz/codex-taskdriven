use crate::ModelRole;
use crate::ReasonCode;
use crate::RoutePin;
use crate::RoutingDecision;
use crate::RoutingSignals;
use crate::TaskDefinition;
use crate::TaskDrivenConfig;
use crate::TaskDrivenConfigError;
use crate::TaskDrivenConfigV2;
use crate::TaskDrivenMode;
use crate::WorkStyle;
use crate::builtin_profile;
use crate::resolve_route;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ReasoningEffort;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskDrivenModel {
    pub model: String,
    pub role: ModelRole,
    pub default_reasoning_effort: ReasoningEffort,
    pub supported_reasoning_efforts: Vec<ReasoningEffort>,
    pub eligible_for_adaptive_routing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedRoute {
    pub phase: String,
    pub model: String,
    pub reasoning_effort: ReasoningEffort,
    pub role: ModelRole,
    pub pinned: bool,
    pub reason_codes: Vec<ReasonCode>,
}

impl TaskDrivenModel {
    pub fn new(
        model: impl Into<String>,
        role: ModelRole,
        default_reasoning_effort: ReasoningEffort,
        supported_reasoning_efforts: Vec<ReasoningEffort>,
    ) -> Self {
        let mut supported_reasoning_efforts = supported_reasoning_efforts;
        if supported_reasoning_efforts.is_empty() {
            supported_reasoning_efforts.push(default_reasoning_effort.clone());
        }
        Self {
            model: model.into(),
            role,
            default_reasoning_effort,
            supported_reasoning_efforts,
            eligible_for_adaptive_routing: true,
        }
    }

    pub fn from_catalog(
        model: impl Into<String>,
        default_reasoning_effort: ReasoningEffort,
        supported_reasoning_efforts: Vec<ReasoningEffort>,
    ) -> Option<Self> {
        let model = model.into();
        let role = classify_model_role(&model)?;
        Some(Self::new(
            model,
            role,
            default_reasoning_effort,
            supported_reasoning_efforts,
        ))
    }

    pub fn from_preset(preset: &ModelPreset) -> Self {
        let classified_role = classify_model_role(&preset.model);
        let mut model = Self::new(
            preset.model.clone(),
            classified_role.unwrap_or(ModelRole::Balanced),
            preset.default_reasoning_effort.clone(),
            preset
                .supported_reasoning_efforts
                .iter()
                .map(|option| option.effort.clone())
                .collect(),
        );
        model.eligible_for_adaptive_routing = classified_role.is_some();
        model
    }

    fn supports(&self, effort: &ReasoningEffort) -> bool {
        self.default_reasoning_effort == *effort
            || self
                .supported_reasoning_efforts
                .iter()
                .any(|supported| supported == effort)
    }
}

pub fn classify_model_role(model: &str) -> Option<ModelRole> {
    let model = model.to_ascii_lowercase();
    if model.ends_with("gpt-6-luna") {
        Some(ModelRole::Efficient)
    } else if model.ends_with("gpt-6.1-sol")
        || model.ends_with("gpt-6-sol")
        || model.ends_with("gpt-5.6-sol")
    {
        Some(ModelRole::Balanced)
    } else if model.ends_with("gpt-6-astra") {
        Some(ModelRole::Frontier)
    } else {
        None
    }
}

impl TaskDrivenConfigV2 {
    pub fn resolve_phase(
        &self,
        phase_id: &str,
        models: &[TaskDrivenModel],
        signals: &RoutingSignals,
    ) -> Result<ResolvedRoute, TaskDrivenConfigError> {
        self.validate()?;
        if !self.phases.iter().any(|phase| phase.id == phase_id) {
            return invalid(format!("phase {phase_id:?} is not configured"));
        }

        match self.mode {
            TaskDrivenMode::Fixed => {
                let pin = self
                    .fixed
                    .as_ref()
                    .and_then(|fixed| fixed.routes.get(phase_id))
                    .ok_or_else(|| {
                        invalid_error(format!("phase {phase_id:?} has no fixed route"))
                    })?;
                resolve_pin(phase_id, pin, models)
            }
            TaskDrivenMode::Adaptive => {
                let adaptive = self.adaptive.as_ref().ok_or_else(|| {
                    invalid_error("adaptive mode requires adaptive settings".to_string())
                })?;
                if let Some(pin) = adaptive.pins.get(phase_id) {
                    return resolve_pin(phase_id, pin, models);
                }
                let profile = builtin_profile_from_reference(&adaptive.profile)?;
                let decision = resolve_route(&profile, phase_id, &adaptive.use_case, signals)
                    .map_err(|error| invalid_error(error.to_string()))?;
                resolve_decision(phase_id, decision, models)
            }
        }
    }

    pub fn materialize(
        &self,
        models: &[TaskDrivenModel],
    ) -> Result<TaskDrivenConfig, TaskDrivenConfigError> {
        self.validate()?;
        let tasks = self
            .phases
            .iter()
            .map(|phase| {
                let route =
                    self.resolve_phase(phase.id.as_str(), models, &RoutingSignals::default())?;
                Ok(TaskDefinition::new(
                    phase.id.clone(),
                    phase.label.clone(),
                    phase.description.clone(),
                    route.model,
                    route.reasoning_effort,
                ))
            })
            .collect::<Result<Vec<_>, TaskDrivenConfigError>>()?;
        let config = TaskDrivenConfig {
            version: 1,
            enabled: self.enabled,
            initial_task: self.initial_phase.clone(),
            tasks,
        };
        config.validate()?;
        Ok(config)
    }
}

fn resolve_pin(
    phase_id: &str,
    pin: &RoutePin,
    models: &[TaskDrivenModel],
) -> Result<ResolvedRoute, TaskDrivenConfigError> {
    let model = models
        .iter()
        .find(|candidate| candidate.model == pin.model)
        .ok_or_else(|| {
            invalid_error(format!(
                "pinned model {} is not in the live catalog",
                pin.model
            ))
        })?;
    if !model.supports(&pin.reasoning_effort) {
        return invalid(format!(
            "pinned route {}/{} is not supported by the live catalog",
            pin.model, pin.reasoning_effort
        ));
    }
    Ok(ResolvedRoute {
        phase: phase_id.to_string(),
        model: model.model.clone(),
        reasoning_effort: pin.reasoning_effort.clone(),
        role: model.role,
        pinned: true,
        reason_codes: Vec::new(),
    })
}

fn resolve_decision(
    phase_id: &str,
    decision: RoutingDecision,
    models: &[TaskDrivenModel],
) -> Result<ResolvedRoute, TaskDrivenConfigError> {
    let model = models
        .iter()
        .filter(|model| model.eligible_for_adaptive_routing)
        .filter(|model| role_rank(model.role) >= role_rank(decision.required_role))
        .min_by_key(|model| (role_rank(model.role), model_preference(&model.model)))
        .ok_or_else(|| {
            invalid_error(format!(
                "the live catalog has no model for required role {:?}",
                decision.required_role
            ))
        })?;
    let reasoning_effort = closest_supported_effort(model, &decision.reasoning_effort)?;
    Ok(ResolvedRoute {
        phase: phase_id.to_string(),
        model: model.model.clone(),
        reasoning_effort,
        role: model.role,
        pinned: false,
        reason_codes: decision.reason_codes,
    })
}

fn closest_supported_effort(
    model: &TaskDrivenModel,
    requested: &ReasoningEffort,
) -> Result<ReasoningEffort, TaskDrivenConfigError> {
    if model.supports(requested) {
        return Ok(requested.clone());
    }
    let requested_rank = effort_rank(requested).ok_or_else(|| {
        invalid_error(format!(
            "unsupported requested reasoning effort {requested}"
        ))
    })?;
    model
        .supported_reasoning_efforts
        .iter()
        .filter_map(|effort| effort_rank(effort).map(|rank| (effort, rank)))
        .min_by_key(|(_, rank)| {
            let below_penalty = u16::from(*rank < requested_rank) * 100;
            below_penalty + u16::from(rank.abs_diff(requested_rank))
        })
        .map(|(effort, _)| effort.clone())
        .ok_or_else(|| {
            invalid_error(format!(
                "model {} has no supported reasoning effort",
                model.model
            ))
        })
}

fn builtin_profile_from_reference(
    reference: &str,
) -> Result<crate::TaskDrivenProfile, TaskDrivenConfigError> {
    let style = match reference {
        "builtin:standard@1" => WorkStyle::Standard,
        "builtin:economy@1" => WorkStyle::Economy,
        "builtin:responsive@1" => WorkStyle::Responsive,
        "builtin:quality@1" => WorkStyle::Quality,
        _ => return invalid(format!("unknown or unavailable profile {reference:?}")),
    };
    Ok(builtin_profile(style))
}

fn role_rank(role: ModelRole) -> u8 {
    match role {
        ModelRole::Efficient => 0,
        ModelRole::Balanced => 1,
        ModelRole::Frontier => 2,
    }
}

fn effort_rank(effort: &ReasoningEffort) -> Option<u8> {
    match effort {
        ReasoningEffort::None => Some(0),
        ReasoningEffort::Minimal => Some(1),
        ReasoningEffort::Low => Some(2),
        ReasoningEffort::Medium => Some(3),
        ReasoningEffort::High => Some(4),
        ReasoningEffort::XHigh => Some(5),
        ReasoningEffort::Max => Some(6),
        ReasoningEffort::Ultra => Some(7),
        ReasoningEffort::Persistent | ReasoningEffort::Custom(_) => None,
    }
}

fn model_preference(model: &str) -> u8 {
    if model.starts_with("gpt-") {
        0
    } else if model.starts_with("openai.") {
        1
    } else {
        2
    }
}

fn invalid<T>(message: impl Into<String>) -> Result<T, TaskDrivenConfigError> {
    Err(invalid_error(message.into()))
}

fn invalid_error(message: String) -> TaskDrivenConfigError {
    TaskDrivenConfigError::Invalid(message)
}
