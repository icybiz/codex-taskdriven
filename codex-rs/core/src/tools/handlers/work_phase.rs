use crate::function_tool::FunctionCallError;
use crate::session::step_settings_are_current;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolCallSource;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_models_manager::ModelsManagerConfig;
use codex_protocol::protocol::TurnSettingsUpdate;
use codex_protocol::protocol::TurnSettingsUpdateOutcome;
use codex_taskdriven::TaskDrivenConfig;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkPhaseArgs {
    phase: String,
    reason: String,
}

pub(crate) struct WorkPhaseHandler {
    config: Arc<TaskDrivenConfig>,
}

impl WorkPhaseHandler {
    pub(crate) fn new(config: TaskDrivenConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }
}

impl ToolExecutor<ToolInvocation> for WorkPhaseHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("set_work_phase")
    }

    fn spec(&self) -> ToolSpec {
        let routes = self
            .config
            .tasks
            .iter()
            .map(|task| {
                format!(
                    "{} ({}) uses {}/{}",
                    task.id, task.label, task.model, task.reasoning_effort
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        ToolSpec::Function(ResponsesApiTool {
            name: "set_work_phase".to_string(),
            description: format!(
                "Select the next TaskDriven task in this conversation. {routes}. Call this tool directly and alone before starting a different task. The selected model applies to the next model request; do not add text or other tools to the switching response."
            ),
            strict: true,
            defer_loading: None,
            parameters: JsonSchema::object(
                BTreeMap::from([
                    (
                        "phase".to_string(),
                        JsonSchema::string_enum(
                            self.config
                                .tasks
                                .iter()
                                .map(|task| json!(task.id))
                                .collect(),
                            Some("Configured TaskDriven task identifier.".to_string()),
                        ),
                    ),
                    (
                        "reason".to_string(),
                        JsonSchema::string(Some(
                            "Brief reason for the switch, at most 300 bytes.".to_string(),
                        )),
                    ),
                ]),
                Some(vec!["phase".to_string(), "reason".to_string()]),
                Some(false.into()),
            ),
            output_schema: None,
        })
    }

    fn handle<'a>(&'a self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'a>
    where
        ToolInvocation: 'a,
    {
        Box::pin(async move {
            let ToolPayload::Function { arguments } = &invocation.payload else {
                return Err(FunctionCallError::RespondToModel(
                    "set_work_phase requires function arguments".to_string(),
                ));
            };
            let args: WorkPhaseArgs = parse_arguments(arguments)?;
            if args.reason.trim().is_empty() || args.reason.len() > 300 {
                return Err(FunctionCallError::RespondToModel(
                    "TaskDriven switch reason must contain 1 to 300 bytes".to_string(),
                ));
            }
            if !matches!(invocation.source, ToolCallSource::Direct)
                || !step_settings_are_current(&invocation.step_context)
                || !codex_taskdriven::instructions_are_enabled(
                    invocation
                        .step_context
                        .settings
                        .effective_collaboration_mode()
                        .settings
                        .developer_instructions
                        .as_deref(),
                )
            {
                return Err(FunctionCallError::RespondToModel(
                    "TaskDriven switching requires a current direct model call with TaskDriven enabled"
                        .to_string(),
                ));
            }
            let Some(task) = self.config.task(&args.phase) else {
                return Err(FunctionCallError::RespondToModel(format!(
                    "TaskDriven task {:?} is not present in this step's configuration",
                    args.phase
                )));
            };
            let current = &invocation.step_context.settings;
            let unchanged = current.model_info.slug == task.model
                && current.effective_reasoning_effort() == Some(task.reasoning_effort.clone());
            if !unchanged {
                let destination = invocation
                    .session
                    .services
                    .models_manager
                    .get_model_info(&task.model, &ModelsManagerConfig::default())
                    .await;
                if destination.used_fallback_model_metadata
                    || !destination
                        .supported_reasoning_levels
                        .iter()
                        .any(|level| level.effort == task.reasoning_effort)
                {
                    return Err(FunctionCallError::RespondToModel(format!(
                        "The live model catalog does not support {}/{}; TaskDriven did not change the route",
                        task.model, task.reasoning_effort
                    )));
                }
                match invocation
                    .session
                    .apply_taskdriven_settings(
                        &invocation.step_context,
                        TurnSettingsUpdate {
                            model: Some(task.model.clone()),
                            effort: Some(Some(task.reasoning_effort.clone())),
                            ..Default::default()
                        },
                    )
                    .await
                {
                    TurnSettingsUpdateOutcome::Applied => {}
                    TurnSettingsUpdateOutcome::TargetUnavailable => {
                        return Err(FunctionCallError::RespondToModel(
                            "The original active turn changed before the TaskDriven switch; no other turn was modified"
                                .to_string(),
                        ));
                    }
                    TurnSettingsUpdateOutcome::Rejected { reason } => {
                        return Err(FunctionCallError::RespondToModel(format!(
                            "TaskDriven switch rejected: {reason}"
                        )));
                    }
                }
            }
            Ok(boxed_tool_output(FunctionToolOutput::from_text(
                json!({
                    "phase": task.id,
                    "label": task.label,
                    "reason": args.reason,
                    "from_model": current.model_info.slug,
                    "model": task.model,
                    "reasoning_effort": task.reasoning_effort,
                    "status": if unchanged {
                        "already_selected"
                    } else {
                        "selected_for_next_model_request"
                    },
                    "thread_id": invocation.session.thread_id().to_string(),
                    "turn_id": invocation.turn.sub_id,
                })
                .to_string(),
                Some(true),
            )))
        })
    }
}

impl CoreToolRuntime for WorkPhaseHandler {
    fn is_builtin_control_tool(&self) -> bool {
        true
    }
}
