use super::TaskDefinition;
use super::TaskDrivenConfig;
use super::TaskDrivenConfigError;
use codex_protocol::openai_models::ReasoningEffort;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use tempfile::NamedTempFile;

pub const TASKDRIVEN_V2_CONFIG_VERSION: u32 = 2;
pub const TASKDRIVEN_PROFILE_SCHEMA_VERSION: u32 = 1;

const MAX_PHASES: usize = 32;
const MAX_ID_BYTES: usize = 64;
const MAX_NAME_BYTES: usize = 80;
const MAX_SUMMARY_BYTES: usize = 300;
const MAX_MODEL_BYTES: usize = 120;
const MAX_REFERENCE_BYTES: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskDrivenMode {
    Fixed,
    Adaptive,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwitchConfirmation {
    #[default]
    Never,
    FrontierEscalation,
    Always,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    Efficient,
    Balanced,
    Frontier,
}

impl ModelRole {
    fn rank(self) -> u8 {
        match self {
            Self::Efficient => 0,
            Self::Balanced => 1,
            Self::Frontier => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkStyle {
    Standard,
    Economy,
    Responsive,
    Quality,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingWeights {
    pub quality: u8,
    pub reliability: u8,
    pub usage: u8,
    pub latency: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingPolicy {
    pub minimum_role: ModelRole,
    pub maximum_role: ModelRole,
    pub initial_effort: ReasoningEffort,
    #[serde(default)]
    pub allow_xhigh: bool,
    #[serde(default)]
    pub allow_max: bool,
    pub escalate_after_failures: u8,
    #[serde(default)]
    pub deescalate_after_successful_phase: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDrivenProfile {
    pub schema_version: u32,
    pub id: String,
    pub version: String,
    pub name: String,
    pub summary: String,
    pub author: String,
    pub license: String,
    pub weights: RoutingWeights,
    pub routing: RoutingPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseDefinition {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutePin {
    pub model: String,
    pub reasoning_effort: ReasoningEffort,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixedRouting {
    pub routes: BTreeMap<String, RoutePin>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdaptiveRouting {
    pub profile: String,
    #[serde(default = "default_use_case")]
    pub use_case: String,
    #[serde(default)]
    pub confirmation: SwitchConfirmation,
    #[serde(default)]
    pub allow_paid_speed_tier: bool,
    #[serde(default)]
    pub pins: BTreeMap<String, RoutePin>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDrivenConfigV2 {
    pub version: u32,
    #[serde(default)]
    pub enabled: bool,
    pub mode: TaskDrivenMode,
    pub initial_phase: String,
    pub phases: Vec<PhaseDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed: Option<FixedRouting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adaptive: Option<AdaptiveRouting>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskDrivenConfigDocument {
    V1(TaskDrivenConfig),
    V2(TaskDrivenConfigV2),
}

#[derive(Deserialize)]
struct VersionProbe {
    version: u32,
}

impl TaskDrivenProfile {
    pub fn validate(&self) -> Result<(), TaskDrivenConfigError> {
        if self.schema_version != TASKDRIVEN_PROFILE_SCHEMA_VERSION {
            return invalid(format!(
                "profile schema_version must be {TASKDRIVEN_PROFILE_SCHEMA_VERSION}, got {}",
                self.schema_version
            ));
        }
        validate_id("profile id", &self.id)?;
        validate_display_text("profile name", &self.name, MAX_NAME_BYTES)?;
        validate_display_text("profile summary", &self.summary, MAX_SUMMARY_BYTES)?;
        validate_display_text("profile author", &self.author, MAX_NAME_BYTES)?;
        validate_token("profile version", &self.version, MAX_ID_BYTES, b".-+")?;
        validate_token("profile license", &self.license, MAX_ID_BYTES, b".-+")?;

        let weights = &self.weights;
        let sum = u16::from(weights.quality)
            + u16::from(weights.reliability)
            + u16::from(weights.usage)
            + u16::from(weights.latency);
        if [
            weights.quality,
            weights.reliability,
            weights.usage,
            weights.latency,
        ]
        .into_iter()
        .any(|weight| weight > 100)
            || sum != 100
        {
            return invalid("profile weights must each be 0..=100 and add up to 100");
        }

        let routing = &self.routing;
        if routing.minimum_role.rank() > routing.maximum_role.rank() {
            return invalid("profile minimum_role must not exceed maximum_role");
        }
        if !(1..=10).contains(&routing.escalate_after_failures) {
            return invalid("profile escalate_after_failures must be between 1 and 10");
        }
        if !profile_effort_is_supported(&routing.initial_effort) {
            return invalid("profile initial_effort must be low, medium, high, xhigh, or max");
        }
        if matches!(
            routing.initial_effort,
            ReasoningEffort::XHigh | ReasoningEffort::Max
        ) && !routing.allow_xhigh
        {
            return invalid("profile initial_effort requires allow_xhigh");
        }
        if matches!(routing.initial_effort, ReasoningEffort::Max) && !routing.allow_max {
            return invalid("profile max initial_effort requires allow_max");
        }
        if routing.allow_max && !routing.allow_xhigh {
            return invalid("profile allow_max requires allow_xhigh");
        }
        Ok(())
    }
}

impl TaskDrivenConfigV2 {
    pub fn adaptive_default() -> Self {
        Self {
            version: TASKDRIVEN_V2_CONFIG_VERSION,
            enabled: false,
            mode: TaskDrivenMode::Adaptive,
            initial_phase: "investigation".to_string(),
            phases: default_phases(),
            fixed: None,
            adaptive: Some(AdaptiveRouting {
                profile: "builtin:standard@1".to_string(),
                use_case: default_use_case(),
                confirmation: SwitchConfirmation::Never,
                allow_paid_speed_tier: false,
                pins: BTreeMap::new(),
            }),
        }
    }

    pub fn from_v1(config: &TaskDrivenConfig) -> Self {
        let phases = config.tasks.iter().map(PhaseDefinition::from).collect();
        let routes = config
            .tasks
            .iter()
            .map(|task| {
                (
                    task.id.clone(),
                    RoutePin {
                        model: task.model.clone(),
                        reasoning_effort: task.reasoning_effort.clone(),
                    },
                )
            })
            .collect();
        Self {
            version: TASKDRIVEN_V2_CONFIG_VERSION,
            enabled: config.enabled,
            mode: TaskDrivenMode::Fixed,
            initial_phase: config.initial_task.clone(),
            phases,
            fixed: Some(FixedRouting { routes }),
            adaptive: None,
        }
    }

    pub fn adaptive_from_v1(config: &TaskDrivenConfig) -> Self {
        let mut migrated = Self::adaptive_default();
        migrated.enabled = config.enabled;
        migrated.initial_phase = config.initial_task.clone();
        migrated.phases = config.tasks.iter().map(PhaseDefinition::from).collect();
        migrated
    }

    pub fn parse(path: &Path, contents: &str) -> Result<Self, TaskDrivenConfigError> {
        let config: Self =
            toml::from_str(contents).map_err(|source| TaskDrivenConfigError::Parse {
                path: path.to_path_buf(),
                source,
            })?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), TaskDrivenConfigError> {
        if self.version != TASKDRIVEN_V2_CONFIG_VERSION {
            return invalid(format!(
                "version must be {TASKDRIVEN_V2_CONFIG_VERSION}, got {}",
                self.version
            ));
        }
        if self.phases.is_empty() || self.phases.len() > MAX_PHASES {
            return invalid(format!(
                "phases must contain between 1 and {MAX_PHASES} entries"
            ));
        }

        let mut phase_ids = HashSet::with_capacity(self.phases.len());
        for phase in &self.phases {
            validate_id("phase id", &phase.id)?;
            if !phase_ids.insert(phase.id.as_str()) {
                return invalid(format!("duplicate phase id {:?}", phase.id));
            }
            validate_display_text("phase label", &phase.label, MAX_NAME_BYTES)?;
            if phase.description.len() > MAX_SUMMARY_BYTES
                || phase.description.chars().any(char::is_control)
            {
                return invalid(format!(
                    "phase {:?} description contains control characters or exceeds {MAX_SUMMARY_BYTES} bytes",
                    phase.id
                ));
            }
        }
        if !phase_ids.contains(self.initial_phase.as_str()) {
            return invalid(format!(
                "initial_phase {:?} does not name a configured phase",
                self.initial_phase
            ));
        }

        match self.mode {
            TaskDrivenMode::Fixed => {
                if self.adaptive.is_some() {
                    return invalid("fixed mode must not contain adaptive settings");
                }
                let Some(fixed) = &self.fixed else {
                    return invalid("fixed mode requires fixed settings");
                };
                if fixed.routes.len() != phase_ids.len()
                    || fixed
                        .routes
                        .keys()
                        .any(|phase| !phase_ids.contains(phase.as_str()))
                {
                    return invalid("fixed mode requires exactly one route for every phase");
                }
                for pin in fixed.routes.values() {
                    validate_pin(pin)?;
                }
            }
            TaskDrivenMode::Adaptive => {
                if self.fixed.is_some() {
                    return invalid("adaptive mode must not contain fixed settings");
                }
                let Some(adaptive) = &self.adaptive else {
                    return invalid("adaptive mode requires adaptive settings");
                };
                validate_reference("adaptive profile", &adaptive.profile)?;
                validate_id("adaptive use_case", &adaptive.use_case)?;
                for (phase, pin) in &adaptive.pins {
                    if !phase_ids.contains(phase.as_str()) {
                        return invalid(format!(
                            "adaptive pin {phase:?} does not name a configured phase"
                        ));
                    }
                    validate_pin(pin)?;
                }
            }
        }
        Ok(())
    }
}

impl TaskDrivenConfigDocument {
    pub fn path(codex_home: &Path) -> PathBuf {
        TaskDrivenConfig::path(codex_home)
    }

    pub fn load(codex_home: &Path) -> Result<Self, TaskDrivenConfigError> {
        let path = Self::path(codex_home);
        match fs::read_to_string(&path) {
            Ok(contents) => Self::parse(&path, &contents),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                let config = TaskDrivenConfigV2::adaptive_default();
                config.validate()?;
                Ok(Self::V2(config))
            }
            Err(source) => Err(TaskDrivenConfigError::Read { path, source }),
        }
    }

    pub fn parse(path: &Path, contents: &str) -> Result<Self, TaskDrivenConfigError> {
        let probe: VersionProbe =
            toml::from_str(contents).map_err(|source| TaskDrivenConfigError::Parse {
                path: path.to_path_buf(),
                source,
            })?;
        match probe.version {
            1 => TaskDrivenConfig::parse(path, contents).map(Self::V1),
            TASKDRIVEN_V2_CONFIG_VERSION => TaskDrivenConfigV2::parse(path, contents).map(Self::V2),
            version => invalid(format!(
                "unsupported TaskDriven configuration version {version}"
            )),
        }
    }

    pub fn into_v2(self) -> TaskDrivenConfigV2 {
        match self {
            Self::V1(config) => TaskDrivenConfigV2::from_v1(&config),
            Self::V2(config) => config,
        }
    }

    pub fn save(&self, codex_home: &Path) -> Result<(), TaskDrivenConfigError> {
        match self {
            Self::V1(config) => config.save(codex_home),
            Self::V2(config) => {
                config.validate()?;
                crate::save_config(codex_home, config)
            }
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Self::V1(config) => config.enabled,
            Self::V2(config) => config.enabled,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        match self {
            Self::V1(config) => config.enabled = enabled,
            Self::V2(config) => config.enabled = enabled,
        }
    }

    pub fn materialize(
        &self,
        models: &[crate::TaskDrivenModel],
    ) -> Result<TaskDrivenConfig, TaskDrivenConfigError> {
        match self {
            Self::V1(config) => Ok(config.clone()),
            Self::V2(config) => config.materialize(models),
        }
    }

    pub fn migrate_v1_to_adaptive(codex_home: &Path) -> Result<Self, TaskDrivenConfigError> {
        let path = Self::path(codex_home);
        let contents = fs::read(&path).map_err(|source| TaskDrivenConfigError::Read {
            path: path.clone(),
            source,
        })?;
        let text = std::str::from_utf8(&contents).map_err(|source| {
            TaskDrivenConfigError::Invalid(format!(
                "{} is not valid UTF-8: {source}",
                path.display()
            ))
        })?;
        let current = Self::parse(&path, text)?;
        let Self::V1(v1) = current else {
            return Ok(current);
        };

        let backup_path = codex_home.join("taskdriven.toml.v1.bak");
        if backup_path.exists() {
            let backup =
                fs::read(&backup_path).map_err(|source| TaskDrivenConfigError::Backup {
                    path: backup_path.clone(),
                    source,
                })?;
            if backup != contents {
                return Err(TaskDrivenConfigError::Invalid(format!(
                    "refusing to replace existing backup {}",
                    backup_path.display()
                )));
            }
        } else {
            let mut temporary = NamedTempFile::new_in(codex_home).map_err(|source| {
                TaskDrivenConfigError::Backup {
                    path: backup_path.clone(),
                    source,
                }
            })?;
            temporary
                .write_all(&contents)
                .and_then(|()| temporary.as_file().sync_all())
                .map_err(|source| TaskDrivenConfigError::Backup {
                    path: backup_path.clone(),
                    source,
                })?;
            temporary.persist_noclobber(&backup_path).map_err(|error| {
                TaskDrivenConfigError::Backup {
                    path: backup_path.clone(),
                    source: error.error,
                }
            })?;
        }

        let migrated = Self::V2(TaskDrivenConfigV2::adaptive_from_v1(&v1));
        migrated.save(codex_home)?;
        Ok(migrated)
    }
}

impl From<&TaskDefinition> for PhaseDefinition {
    fn from(task: &TaskDefinition) -> Self {
        Self {
            id: task.id.clone(),
            label: task.label.clone(),
            description: task.description.clone(),
        }
    }
}

pub fn builtin_profiles() -> [TaskDrivenProfile; 4] {
    [
        builtin_profile(WorkStyle::Standard),
        builtin_profile(WorkStyle::Economy),
        builtin_profile(WorkStyle::Responsive),
        builtin_profile(WorkStyle::Quality),
    ]
}

pub fn builtin_profile(style: WorkStyle) -> TaskDrivenProfile {
    let (id, name, summary, weights, initial_effort, allow_xhigh, allow_max, failures) = match style
    {
        WorkStyle::Standard => (
            "standard",
            "Standard",
            "Balanced automatic routing for most work.",
            RoutingWeights {
                quality: 35,
                reliability: 30,
                usage: 20,
                latency: 15,
            },
            ReasoningEffort::Medium,
            true,
            false,
            2,
        ),
        WorkStyle::Economy => (
            "economy",
            "Economy",
            "Use the lightest sufficient route and escalate on concrete evidence.",
            RoutingWeights {
                quality: 20,
                reliability: 30,
                usage: 40,
                latency: 10,
            },
            ReasoningEffort::Low,
            false,
            false,
            2,
        ),
        WorkStyle::Responsive => (
            "responsive",
            "Responsive",
            "Prefer short interactive steps and low initial latency.",
            RoutingWeights {
                quality: 20,
                reliability: 25,
                usage: 15,
                latency: 40,
            },
            ReasoningEffort::Low,
            false,
            false,
            1,
        ),
        WorkStyle::Quality => (
            "quality",
            "Quality focus",
            "Escalate earlier and spend more effort on verification.",
            RoutingWeights {
                quality: 40,
                reliability: 40,
                usage: 10,
                latency: 10,
            },
            ReasoningEffort::Medium,
            true,
            false,
            1,
        ),
    };
    TaskDrivenProfile {
        schema_version: TASKDRIVEN_PROFILE_SCHEMA_VERSION,
        id: id.to_string(),
        version: "1.0.0".to_string(),
        name: name.to_string(),
        summary: summary.to_string(),
        author: "icybiz".to_string(),
        license: "Apache-2.0".to_string(),
        weights,
        routing: RoutingPolicy {
            minimum_role: ModelRole::Efficient,
            maximum_role: ModelRole::Frontier,
            initial_effort,
            allow_xhigh,
            allow_max,
            escalate_after_failures: failures,
            deescalate_after_successful_phase: true,
        },
    }
}

fn default_phases() -> Vec<PhaseDefinition> {
    vec![
        PhaseDefinition {
            id: "writing".to_string(),
            label: "Writing".to_string(),
            description: "Drafting, rewriting, and user-facing prose".to_string(),
        },
        PhaseDefinition {
            id: "investigation".to_string(),
            label: "Investigation".to_string(),
            description: "Reading, diagnosis, research, and design".to_string(),
        },
        PhaseDefinition {
            id: "implementation".to_string(),
            label: "Implementation".to_string(),
            description: "Code changes, fixes, and test authoring".to_string(),
        },
        PhaseDefinition {
            id: "verification".to_string(),
            label: "Verification".to_string(),
            description: "Builds, tests, reviews, and result reporting".to_string(),
        },
    ]
}

fn default_use_case() -> String {
    "auto".to_string()
}

fn profile_effort_is_supported(effort: &ReasoningEffort) -> bool {
    matches!(
        effort,
        ReasoningEffort::Low
            | ReasoningEffort::Medium
            | ReasoningEffort::High
            | ReasoningEffort::XHigh
            | ReasoningEffort::Max
    )
}

fn validate_pin(pin: &RoutePin) -> Result<(), TaskDrivenConfigError> {
    validate_display_text("pinned model", &pin.model, MAX_MODEL_BYTES)
}

fn validate_id(field: &str, value: &str) -> Result<(), TaskDrivenConfigError> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > MAX_ID_BYTES
        || !bytes[0].is_ascii_lowercase()
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(byte))
    {
        return invalid(format!(
            "{field} must start with a lowercase letter and contain at most {MAX_ID_BYTES} lowercase letters, digits, '-' or '_'"
        ));
    }
    Ok(())
}

fn validate_reference(field: &str, value: &str) -> Result<(), TaskDrivenConfigError> {
    validate_token(field, value, MAX_REFERENCE_BYTES, b":/@._-+")
}

fn validate_token(
    field: &str,
    value: &str,
    max_bytes: usize,
    punctuation: &[u8],
) -> Result<(), TaskDrivenConfigError> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > max_bytes
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || punctuation.contains(byte))
    {
        return invalid(format!(
            "{field} contains unsupported characters or exceeds {max_bytes} bytes"
        ));
    }
    Ok(())
}

fn validate_display_text(
    field: &str,
    value: &str,
    max_bytes: usize,
) -> Result<(), TaskDrivenConfigError> {
    if value.trim().is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return invalid(format!(
            "{field} must contain between 1 and {max_bytes} bytes without control characters"
        ));
    }
    Ok(())
}

fn invalid<T>(message: impl Into<String>) -> Result<T, TaskDrivenConfigError> {
    Err(TaskDrivenConfigError::Invalid(message.into()))
}
