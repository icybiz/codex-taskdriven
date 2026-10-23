//! Versioned, external TaskDriven routing configuration shared by Codex surfaces.

mod adaptive;
mod catalog;
mod router;

pub use adaptive::AdaptiveRouting;
pub use adaptive::FixedRouting;
pub use adaptive::ModelRole;
pub use adaptive::PhaseDefinition;
pub use adaptive::RoutePin;
pub use adaptive::RoutingPolicy;
pub use adaptive::RoutingWeights;
pub use adaptive::SwitchConfirmation;
pub use adaptive::TaskDrivenConfigDocument;
pub use adaptive::TaskDrivenConfigV2;
pub use adaptive::TaskDrivenMode;
pub use adaptive::TaskDrivenProfile;
pub use adaptive::WorkStyle;
pub use adaptive::builtin_profile;
pub use adaptive::builtin_profiles;
pub use catalog::ResolvedRoute;
pub use catalog::TaskDrivenModel;
pub use catalog::classify_model_role;
pub use router::BuiltinUseCase;
pub use router::ReasonCode;
pub use router::RoutingDecision;
pub use router::RoutingError;
pub use router::RoutingSignals;
pub use router::resolve_route;

use codex_protocol::openai_models::ReasoningEffort;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use tempfile::NamedTempFile;
use thiserror::Error;

pub const CONFIG_FILE_NAME: &str = "taskdriven.toml";
pub const INSTRUCTIONS_BEGIN: &str = "<taskdriven_routing>";
pub const INSTRUCTIONS_END: &str = "</taskdriven_routing>";
const CONFIG_VERSION: u32 = 1;
const MAX_TASKS: usize = 32;
const MAX_LABEL_BYTES: usize = 80;
const MAX_DESCRIPTION_BYTES: usize = 300;
const MAX_MODEL_BYTES: usize = 120;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDrivenConfig {
    pub version: u32,
    #[serde(default)]
    pub enabled: bool,
    pub initial_task: String,
    pub tasks: Vec<TaskDefinition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDefinition {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    pub model: String,
    pub reasoning_effort: ReasoningEffort,
}

#[derive(Debug, Error)]
pub enum TaskDrivenConfigError {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not parse {path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("invalid TaskDriven configuration: {0}")]
    Invalid(String),
    #[error("could not serialize TaskDriven configuration: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("could not create a temporary configuration beside {path}: {source}")]
    CreateTemporary {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not write the temporary configuration for {path}: {source}")]
    WriteTemporary {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not replace {path}: {source}")]
    Persist {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not preserve the v1 configuration at {path}: {source}")]
    Backup {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl Default for TaskDrivenConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            enabled: false,
            initial_task: "investigation".to_string(),
            tasks: vec![
                TaskDefinition::new(
                    "writing",
                    "Writing",
                    "Drafting, rewriting, and user-facing prose",
                    "gpt-5.6-sol",
                    ReasoningEffort::XHigh,
                ),
                TaskDefinition::new(
                    "investigation",
                    "Investigation",
                    "Reading, diagnosis, research, and design",
                    "gpt-5.6-sol",
                    ReasoningEffort::XHigh,
                ),
                TaskDefinition::new(
                    "implementation",
                    "Implementation",
                    "Code changes, fixes, and test authoring",
                    "gpt-6-astra",
                    ReasoningEffort::Max,
                ),
                TaskDefinition::new(
                    "verification",
                    "Verification",
                    "Builds, tests, reviews, and result reporting",
                    "gpt-5.6-sol",
                    ReasoningEffort::XHigh,
                ),
            ],
        }
    }
}

impl TaskDefinition {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        description: impl Into<String>,
        model: impl Into<String>,
        reasoning_effort: ReasoningEffort,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: description.into(),
            model: model.into(),
            reasoning_effort,
        }
    }
}

impl TaskDrivenConfig {
    pub fn path(codex_home: &Path) -> PathBuf {
        codex_home.join(CONFIG_FILE_NAME)
    }

    pub fn load(codex_home: &Path) -> Result<Self, TaskDrivenConfigError> {
        let path = Self::path(codex_home);
        match fs::read_to_string(&path) {
            Ok(contents) => Self::parse(&path, &contents),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                let config = Self::default();
                config.validate()?;
                Ok(config)
            }
            Err(source) => Err(TaskDrivenConfigError::Read { path, source }),
        }
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

    pub fn save(&self, codex_home: &Path) -> Result<(), TaskDrivenConfigError> {
        self.validate()?;
        save_config(codex_home, self)
    }

    pub fn validate(&self) -> Result<(), TaskDrivenConfigError> {
        if self.version != CONFIG_VERSION {
            return Err(TaskDrivenConfigError::Invalid(format!(
                "version must be {CONFIG_VERSION}, got {}",
                self.version
            )));
        }
        if self.tasks.is_empty() || self.tasks.len() > MAX_TASKS {
            return Err(TaskDrivenConfigError::Invalid(format!(
                "tasks must contain between 1 and {MAX_TASKS} entries"
            )));
        }
        let mut ids = HashSet::with_capacity(self.tasks.len());
        for task in &self.tasks {
            if !valid_task_id(&task.id) {
                return Err(TaskDrivenConfigError::Invalid(format!(
                    "task id {:?} must start with a lowercase letter and contain at most 64 lowercase letters, digits, '-' or '_'",
                    task.id
                )));
            }
            if !ids.insert(task.id.as_str()) {
                return Err(TaskDrivenConfigError::Invalid(format!(
                    "duplicate task id {:?}",
                    task.id
                )));
            }
            validate_text(&task.id, "label", &task.label, MAX_LABEL_BYTES)?;
            validate_text(&task.id, "model", &task.model, MAX_MODEL_BYTES)?;
            if task.description.len() > MAX_DESCRIPTION_BYTES {
                return Err(TaskDrivenConfigError::Invalid(format!(
                    "{}.description exceeds {MAX_DESCRIPTION_BYTES} bytes",
                    task.id
                )));
            }
        }
        if !ids.contains(self.initial_task.as_str()) {
            return Err(TaskDrivenConfigError::Invalid(format!(
                "initial_task {:?} does not name a configured task",
                self.initial_task
            )));
        }
        Ok(())
    }

    pub fn initial(&self) -> Option<&TaskDefinition> {
        self.task(&self.initial_task)
    }

    pub fn task(&self, id: &str) -> Option<&TaskDefinition> {
        self.tasks.iter().find(|task| task.id == id)
    }

    pub fn task_mut(&mut self, id: &str) -> Option<&mut TaskDefinition> {
        self.tasks.iter_mut().find(|task| task.id == id)
    }

    pub fn instructions_block(&self) -> String {
        let routes = self
            .tasks
            .iter()
            .map(|task| {
                let suffix = if task.description.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", task.description)
                };
                format!(
                    "- `{}` ({}): {}/{}{}",
                    task.id, task.label, task.model, task.reasoning_effort, suffix
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "{INSTRUCTIONS_BEGIN}\nTaskDriven routing is active. Available tasks:\n{routes}\nBefore performing work belonging to a different task, call `set_work_phase` directly as the only tool call in that response, then continue after its result. A successful switch applies to the next model request in this same thread. Do not add text or other tools to the switching response. If a switch is rejected, report the reason and do not silently continue with the wrong route. Routing never changes permissions or approval requirements.\n{INSTRUCTIONS_END}"
        )
    }
}

pub(crate) fn save_config<T: Serialize + ?Sized>(
    codex_home: &Path,
    config: &T,
) -> Result<(), TaskDrivenConfigError> {
    let path = TaskDrivenConfig::path(codex_home);
    fs::create_dir_all(codex_home).map_err(|source| TaskDrivenConfigError::WriteTemporary {
        path: path.clone(),
        source,
    })?;
    let contents = toml::to_string_pretty(config)?;
    let mut temporary = NamedTempFile::new_in(codex_home).map_err(|source| {
        TaskDrivenConfigError::CreateTemporary {
            path: path.clone(),
            source,
        }
    })?;
    temporary
        .write_all(contents.as_bytes())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| TaskDrivenConfigError::WriteTemporary {
            path: path.clone(),
            source,
        })?;
    temporary
        .persist(&path)
        .map_err(|error| TaskDrivenConfigError::Persist {
            path,
            source: error.error,
        })?;
    Ok(())
}

pub fn instructions_are_enabled(instructions: Option<&str>) -> bool {
    instructions.is_some_and(|instructions| {
        instructions.contains(INSTRUCTIONS_BEGIN) && instructions.contains(INSTRUCTIONS_END)
    })
}

pub fn enable_instructions(existing: Option<&str>, config: &TaskDrivenConfig) -> String {
    let base = strip_instructions(existing).unwrap_or_default();
    if base.is_empty() {
        config.instructions_block()
    } else {
        format!("{base}\n\n{}", config.instructions_block())
    }
}

pub fn strip_instructions(existing: Option<&str>) -> Option<String> {
    let mut text = existing.unwrap_or_default().to_string();
    while let Some(start) = text.find(INSTRUCTIONS_BEGIN) {
        let Some(relative_end) = text[start..].find(INSTRUCTIONS_END) else {
            text.truncate(start);
            break;
        };
        let end = start + relative_end + INSTRUCTIONS_END.len();
        text.replace_range(start..end, "");
    }
    let trimmed = text.trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn valid_task_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(byte))
}

fn validate_text(
    task_id: &str,
    field: &str,
    value: &str,
    max_bytes: usize,
) -> Result<(), TaskDrivenConfigError> {
    if value.trim().is_empty() || value.len() > max_bytes {
        return Err(TaskDrivenConfigError::Invalid(format!(
            "{task_id}.{field} must contain between 1 and {max_bytes} bytes"
        )));
    }
    Ok(())
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "adaptive_tests.rs"]
mod adaptive_tests;

#[cfg(test)]
#[path = "router_tests.rs"]
mod router_tests;
