//! Editable TaskDriven routing backed by an external configuration file.

use super::*;
use codex_protocol::config_types::ModelRouting;
use codex_taskdriven::PhaseDefinition;
use codex_taskdriven::RoutePin;
use codex_taskdriven::TaskDefinition;
use codex_taskdriven::TaskDrivenConfig;
use codex_taskdriven::TaskDrivenConfigDocument;
use codex_taskdriven::TaskDrivenConfigV2;
use codex_taskdriven::TaskDrivenMode;
use codex_taskdriven::TaskDrivenModel;

const TASKDRIVEN_SELECTION_VIEW_ID: &str = "taskdriven-selection";
const TASKDRIVEN_USE_CASES: [(&str, &str, &str); 8] = [
    (
        "auto",
        "Automatic",
        "Infer a stable general-purpose emphasis from the work.",
    ),
    (
        "fix",
        "Find and fix a bug",
        "Emphasize diagnosis, regressions, and verification.",
    ),
    (
        "build",
        "Build something new",
        "Emphasize architecture, implementation, and completion.",
    ),
    (
        "web_app",
        "Build a website or web app",
        "Emphasize full-stack and browser verification.",
    ),
    (
        "data_automation",
        "Analyze data or automate work",
        "Emphasize integrity, repeatability, and edge cases.",
    ),
    (
        "interactive_game",
        "Build an interactive app or game",
        "Emphasize interaction, visuals, and runtime behavior.",
    ),
    (
        "maintain",
        "Improve an existing project",
        "Emphasize compatibility and regression control.",
    ),
    (
        "research",
        "Research and prepare a decision",
        "Emphasize source quality, uncertainty, and synthesis.",
    ),
];

impl ChatWidget {
    pub(crate) fn taskdriven_selected(&self) -> bool {
        self.current_collaboration_mode.settings.model_routing == ModelRouting::TaskDriven
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn model_selection_name(&self) -> &str {
        if self.taskdriven_selected() {
            "taskdriven"
        } else {
            self.current_model()
        }
    }

    pub(super) fn taskdriven_selection_item(
        &self,
        presets: &[ModelPreset],
    ) -> Option<SelectionItem> {
        if !self.config.features.enabled(Feature::AutonomousWorkPhase)
            || !self.config.features.enabled(Feature::StepModelSwitching)
        {
            return None;
        }
        let document = match TaskDrivenConfigDocument::load(self.config.codex_home.as_path()) {
            Ok(document) => document,
            Err(error) => {
                return Some(SelectionItem {
                    name: "TaskDriven".to_string(),
                    description: Some(error.to_string()),
                    is_disabled: true,
                    disabled_reason: Some(
                        "Fix $CODEX_HOME/taskdriven.toml and reopen /model".to_string(),
                    ),
                    ..Default::default()
                });
            }
        };
        let runtime = self.materialize_taskdriven_document(&document, presets);
        let catalog_note = runtime.as_ref().err().cloned();
        let summary = match (&document, runtime.as_ref().ok()) {
            (TaskDrivenConfigDocument::V1(config), Some(runtime)) => {
                let initial = runtime.initial()?;
                format!(
                    "Route {} editable tasks by phase · starts with {}/{}",
                    config.tasks.len(),
                    initial.model,
                    initial.reasoning_effort
                )
            }
            (TaskDrivenConfigDocument::V2(config), Some(runtime)) => {
                let initial = runtime.initial()?;
                match config.mode {
                    TaskDrivenMode::Adaptive => format!(
                        "{} · {} · starts with {}/{}",
                        taskdriven_profile_name(config),
                        taskdriven_use_case_name(config),
                        initial.model,
                        initial.reasoning_effort
                    ),
                    TaskDrivenMode::Fixed => format!(
                        "Fixed expert routes · starts with {}/{}",
                        initial.model, initial.reasoning_effort
                    ),
                }
            }
            (_, None) => "TaskDriven configuration needs attention".to_string(),
        };
        Some(SelectionItem {
            name: match &document {
                TaskDrivenConfigDocument::V2(config) if config.mode == TaskDrivenMode::Adaptive => {
                    format!("TaskDriven · {}", taskdriven_profile_name(config))
                }
                _ => "TaskDriven".to_string(),
            },
            description: Some(catalog_note.unwrap_or(summary)),
            is_current: self.taskdriven_selected(),
            actions: vec![Box::new(|tx| tx.send(AppEvent::OpenTaskDrivenPopup))],
            dismiss_on_select: true,
            ..Default::default()
        })
    }

    pub(crate) fn open_taskdriven_popup(&mut self) {
        let document = match self.load_taskdriven_document() {
            Ok(document) => document,
            Err(error) => {
                self.add_error_message(error);
                return;
            }
        };
        let is_v1 = matches!(&document, TaskDrivenConfigDocument::V1(_));
        let selected = self.taskdriven_selected();
        let mut items = vec![SelectionItem {
            name: if selected {
                "Disable TaskDriven"
            } else {
                "Enable TaskDriven"
            }
            .to_string(),
            description: Some(
                if selected {
                    "Keep the active model and return to fixed model selection."
                } else {
                    "Start with the configured initial task and route later phases automatically."
                }
                .to_string(),
            ),
            actions: vec![Box::new(move |tx| {
                tx.send(AppEvent::SetTaskDrivenEnabled(!selected));
            })],
            dismiss_on_select: true,
            ..Default::default()
        }];
        if let TaskDrivenConfigDocument::V2(config) = &document
            && config.mode == TaskDrivenMode::Adaptive
        {
            items.extend([
                SelectionItem {
                    name: format!("Work style · {}", taskdriven_profile_name(config)),
                    description: Some(
                        "Balance quality, reliability, usage, and latency.".to_string(),
                    ),
                    actions: vec![Box::new(|tx| {
                        tx.send(AppEvent::OpenTaskDrivenProfilePopup);
                    })],
                    dismiss_on_select: true,
                    ..Default::default()
                },
                SelectionItem {
                    name: format!("Use case · {}", taskdriven_use_case_name(config)),
                    description: Some(
                        "Choose what kind of work the router should emphasize.".to_string(),
                    ),
                    actions: vec![Box::new(|tx| {
                        tx.send(AppEvent::OpenTaskDrivenUseCasePopup);
                    })],
                    dismiss_on_select: true,
                    ..Default::default()
                },
            ]);
        }

        let presets = self.model_catalog.try_list_models().unwrap_or_default();
        let runtime = self
            .materialize_taskdriven_document(&document, &presets)
            .ok();
        for phase in taskdriven_phases(&document) {
            let phase_id = phase.id.clone();
            let route = runtime.as_ref().and_then(|config| config.task(&phase.id));
            let route_description = route.map_or_else(
                || "Route unavailable; choose an exact model to repair it".to_string(),
                |task| {
                    let pin = taskdriven_phase_is_pinned(&document, &phase.id);
                    format!(
                        "{}/{}{}{}",
                        task.model,
                        task.reasoning_effort,
                        if is_v1 {
                            ""
                        } else if pin {
                            " · pinned"
                        } else {
                            " · automatic"
                        },
                        if phase.description.is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", phase.description)
                        }
                    )
                },
            );
            items.push(SelectionItem {
                name: format!("{} · {}", phase.label, phase.id),
                description: Some(route_description),
                is_current: phase.id == taskdriven_initial_phase(&document),
                actions: vec![Box::new(move |tx| {
                    tx.send(AppEvent::OpenTaskDrivenModelPopup {
                        task_id: phase_id.clone(),
                    });
                })],
                dismiss_on_select: true,
                ..Default::default()
            });
        }
        let phase_count = taskdriven_phases(&document).len();
        items.extend([
            SelectionItem {
                name: if is_v1 {
                    "Choose initial task…"
                } else {
                    "Choose initial phase…"
                }
                .to_string(),
                description: Some("Select the route used when TaskDriven is enabled.".to_string()),
                actions: vec![Box::new(|tx| {
                    tx.send(AppEvent::OpenTaskDrivenInitialTaskPopup);
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: if is_v1 { "Add task…" } else { "Add phase…" }.to_string(),
                description: Some(
                    if is_v1 {
                        "Create a task, then choose its model and effort."
                    } else {
                        "Create a phase, then choose or pin its route."
                    }
                    .to_string(),
                ),
                actions: vec![Box::new(|tx| {
                    tx.send(AppEvent::OpenTaskDrivenAddPrompt);
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: if is_v1 {
                    "Remove task…"
                } else {
                    "Remove phase…"
                }
                .to_string(),
                description: Some(
                    if is_v1 {
                        "Delete a configured task."
                    } else {
                        "Delete a configured phase."
                    }
                    .to_string(),
                ),
                is_disabled: phase_count == 1,
                disabled_reason: (phase_count == 1).then_some(
                    if is_v1 {
                        "The last task cannot be removed"
                    } else {
                        "The last phase cannot be removed"
                    }
                    .to_string(),
                ),
                actions: vec![Box::new(|tx| {
                    tx.send(AppEvent::OpenTaskDrivenRemovePopup);
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
        ]);
        if is_v1 {
            items.push(SelectionItem {
                name: "Switch to adaptive Standard…".to_string(),
                description: Some(
                    "Preserve an exact v1 backup, then use profiles and automatic routes."
                        .to_string(),
                ),
                actions: vec![Box::new(|tx| {
                    tx.send(AppEvent::MigrateTaskDrivenToAdaptive);
                })],
                dismiss_on_select: true,
                require_explicit_confirmation: true,
                ..Default::default()
            });
        }
        let mut header = ColumnRenderable::new();
        header.push(Line::from(match &document {
            TaskDrivenConfigDocument::V2(config) if config.mode == TaskDrivenMode::Adaptive => {
                format!("TaskDriven · {}", taskdriven_profile_name(config)).bold()
            }
            _ => "TaskDriven routing".bold(),
        }));
        header.push(Line::from("Stored in $CODEX_HOME/taskdriven.toml".dim()));
        self.bottom_pane.show_selection_view(SelectionViewParams {
            view_id: Some(TASKDRIVEN_SELECTION_VIEW_ID),
            header: Box::new(header),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    pub(crate) fn open_taskdriven_profile_popup(&mut self) {
        let document = match self.load_taskdriven_document() {
            Ok(document) => document,
            Err(error) => {
                self.add_error_message(error);
                return;
            }
        };
        let TaskDrivenConfigDocument::V2(config) = document else {
            self.add_error_message("Adaptive profiles require TaskDriven v2".to_string());
            return;
        };
        let Some(adaptive) = config.adaptive else {
            self.add_error_message("TaskDriven is not in adaptive mode".to_string());
            return;
        };
        let items = codex_taskdriven::builtin_profiles()
            .into_iter()
            .map(|profile| {
                let reference = format!("builtin:{}@1", profile.id);
                let selected = reference == adaptive.profile;
                SelectionItem {
                    name: profile.name,
                    description: Some(profile.summary),
                    is_current: selected,
                    actions: vec![Box::new(move |tx| {
                        tx.send(AppEvent::SetTaskDrivenProfile(reference.clone()));
                    })],
                    dismiss_on_select: true,
                    ..Default::default()
                }
            })
            .collect();
        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("TaskDriven work style".to_string()),
            subtitle: Some(
                "Profiles change model and reasoning preferences, not permissions.".to_string(),
            ),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    pub(crate) fn open_taskdriven_use_case_popup(&mut self) {
        let document = match self.load_taskdriven_document() {
            Ok(document) => document,
            Err(error) => {
                self.add_error_message(error);
                return;
            }
        };
        let TaskDrivenConfigDocument::V2(config) = document else {
            self.add_error_message("Use cases require TaskDriven v2".to_string());
            return;
        };
        let Some(adaptive) = config.adaptive else {
            self.add_error_message("TaskDriven is not in adaptive mode".to_string());
            return;
        };
        let items = TASKDRIVEN_USE_CASES
            .into_iter()
            .map(|(id, name, description)| {
                let id = id.to_string();
                SelectionItem {
                    name: name.to_string(),
                    description: Some(description.to_string()),
                    is_current: id == adaptive.use_case,
                    actions: vec![Box::new(move |tx| {
                        tx.send(AppEvent::SetTaskDrivenUseCase(id.clone()));
                    })],
                    dismiss_on_select: true,
                    ..Default::default()
                }
            })
            .collect();
        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("TaskDriven use case".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    pub(crate) fn open_taskdriven_model_popup(&mut self, task_id: String) {
        let document = match self.load_taskdriven_document() {
            Ok(document) => document,
            Err(error) => {
                self.add_error_message(error);
                return;
            }
        };
        let Some(phase) = taskdriven_phases(&document)
            .into_iter()
            .find(|phase| phase.id == task_id)
        else {
            self.add_error_message(format!("TaskDriven phase {task_id:?} no longer exists"));
            return;
        };
        let current_model = self
            .model_catalog
            .try_list_models()
            .ok()
            .and_then(|presets| {
                self.materialize_taskdriven_document(&document, &presets)
                    .ok()
            })
            .and_then(|runtime| runtime.task(&task_id).map(|task| task.model.clone()));
        let items = self
            .model_catalog
            .try_list_models()
            .unwrap_or_default()
            .into_iter()
            .filter(|preset| preset.show_in_picker)
            .map(|preset| {
                let preset_for_action = preset.clone();
                let task_id = task_id.clone();
                SelectionItem {
                    name: preset.model.clone(),
                    description: (!preset.description.is_empty())
                        .then_some(preset.description.clone()),
                    is_current: current_model.as_deref() == Some(preset.model.as_str()),
                    actions: vec![Box::new(move |tx| {
                        tx.send(AppEvent::OpenTaskDrivenEffortPopup {
                            task_id: task_id.clone(),
                            model: preset_for_action.clone(),
                        });
                    })],
                    dismiss_on_select: true,
                    ..Default::default()
                }
            })
            .collect();
        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some(format!("Model for {}", phase.label)),
            subtitle: Some(
                if matches!(
                    document,
                    TaskDrivenConfigDocument::V2(TaskDrivenConfigV2 {
                        mode: TaskDrivenMode::Adaptive,
                        ..
                    })
                ) {
                    "Choose an exact model pin from the live catalog."
                } else {
                    "Choose from the live model catalog."
                }
                .to_string(),
            ),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            is_searchable: true,
            search_placeholder: Some("Type to search models".to_string()),
            ..Default::default()
        });
    }

    pub(crate) fn open_taskdriven_effort_popup(&mut self, task_id: String, preset: ModelPreset) {
        let document = match self.load_taskdriven_document() {
            Ok(document) => document,
            Err(error) => {
                self.add_error_message(error);
                return;
            }
        };
        let Some(phase) = taskdriven_phases(&document)
            .into_iter()
            .find(|phase| phase.id == task_id)
        else {
            self.add_error_message(format!("TaskDriven phase {task_id:?} no longer exists"));
            return;
        };
        let current_route = self
            .model_catalog
            .try_list_models()
            .ok()
            .and_then(|presets| {
                self.materialize_taskdriven_document(&document, &presets)
                    .ok()
            })
            .and_then(|runtime| runtime.task(&task_id).cloned());
        let mut efforts = preset
            .supported_reasoning_efforts
            .iter()
            .map(|option| option.effort.clone())
            .collect::<Vec<_>>();
        if efforts.is_empty() {
            efforts.push(preset.default_reasoning_effort.clone());
        }
        let items = efforts
            .into_iter()
            .map(|effort| {
                let task_id = task_id.clone();
                let model = preset.model.clone();
                let selected = current_route
                    .as_ref()
                    .is_some_and(|task| task.model == model && task.reasoning_effort == effort);
                SelectionItem {
                    name: Self::reasoning_effort_label(&effort),
                    is_current: selected,
                    actions: vec![Box::new(move |tx| {
                        tx.send(AppEvent::SetTaskDrivenMapping {
                            task_id: task_id.clone(),
                            model: model.clone(),
                            effort: effort.clone(),
                        });
                    })],
                    dismiss_on_select: true,
                    ..Default::default()
                }
            })
            .collect();
        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some(format!("Reasoning for {}", phase.label)),
            subtitle: Some(format!("Model: {}", preset.model)),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    pub(crate) fn open_taskdriven_initial_task_popup(&mut self) {
        let document = match self.load_taskdriven_document() {
            Ok(document) => document,
            Err(error) => {
                self.add_error_message(error);
                return;
            }
        };
        let initial_phase = taskdriven_initial_phase(&document).to_string();
        let presets = self.model_catalog.try_list_models().unwrap_or_default();
        let runtime = self
            .materialize_taskdriven_document(&document, &presets)
            .ok();
        let items = taskdriven_phases(&document)
            .into_iter()
            .map(|phase| {
                let task_id = phase.id.clone();
                let description = runtime
                    .as_ref()
                    .and_then(|config| config.task(&phase.id))
                    .map(|task| format!("{}/{}", task.model, task.reasoning_effort));
                SelectionItem {
                    name: phase.label,
                    description,
                    is_current: phase.id == initial_phase,
                    actions: vec![Box::new(move |tx| {
                        tx.send(AppEvent::SetTaskDrivenInitialTask(task_id.clone()));
                    })],
                    dismiss_on_select: true,
                    ..Default::default()
                }
            })
            .collect();
        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Initial TaskDriven phase".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    pub(crate) fn open_taskdriven_add_prompt(&mut self) {
        let tx = self.app_event_tx.clone();
        let view = CustomPromptView::new(
            "Add TaskDriven phase".to_string(),
            "phase-id | Label | Description".to_string(),
            String::new(),
            Some("The new phase initially uses the active model and effort.".to_string()),
            Box::new(move |spec| tx.send(AppEvent::AddTaskDrivenTask(spec))),
        );
        self.bottom_pane.show_text_prompt(view);
    }

    pub(crate) fn open_taskdriven_remove_popup(&mut self) {
        let document = match self.load_taskdriven_document() {
            Ok(document) => document,
            Err(error) => {
                self.add_error_message(error);
                return;
            }
        };
        let items = taskdriven_phases(&document)
            .into_iter()
            .map(|phase| {
                let task_id = phase.id.clone();
                SelectionItem {
                    name: phase.label,
                    description: Some(phase.id),
                    actions: vec![Box::new(move |tx| {
                        tx.send(AppEvent::RemoveTaskDrivenTask(task_id.clone()));
                    })],
                    dismiss_on_select: true,
                    require_explicit_confirmation: true,
                    ..Default::default()
                }
            })
            .collect();
        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Remove TaskDriven phase".to_string()),
            subtitle: Some("This changes the external routing configuration.".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    pub(crate) fn set_taskdriven_enabled(
        &self,
        enabled: bool,
    ) -> Result<CollaborationMode, String> {
        if enabled
            && (!self.config.features.enabled(Feature::AutonomousWorkPhase)
                || !self.config.features.enabled(Feature::StepModelSwitching))
        {
            return Err("Automatic TaskDriven routing is unavailable in this session".to_string());
        }
        let runtime = if enabled {
            let document = self.load_taskdriven_document()?;
            let presets = self.model_catalog.try_list_models().map_err(|_| {
                "Models are being updated; try enabling TaskDriven again in a moment".to_string()
            })?;
            Some(self.materialize_taskdriven_document(&document, &presets)?)
        } else {
            None
        };
        // Activation belongs to the thread. Persist only routing/profile edits,
        // never a model-picker toggle that would affect other sessions.
        let mut mode = self.effective_collaboration_mode();
        if enabled {
            let config = runtime.expect("enabled TaskDriven has a materialized configuration");
            let initial = config.initial().ok_or_else(|| {
                "TaskDriven initial phase no longer names a configured phase".to_string()
            })?;
            mode.settings.model = initial.model.clone();
            mode.settings.reasoning_effort = Some(initial.reasoning_effort.clone());
            mode.settings.model_routing = ModelRouting::TaskDriven;
            mode.settings.developer_instructions = Some(codex_taskdriven::enable_instructions(
                mode.settings.developer_instructions.as_deref(),
                &config,
            ));
        } else {
            mode.settings.model_routing = ModelRouting::Fixed;
            mode.settings.developer_instructions = codex_taskdriven::strip_instructions(
                mode.settings.developer_instructions.as_deref(),
            );
        }
        Ok(mode)
    }

    pub(crate) fn set_taskdriven_mapping(
        &self,
        task_id: &str,
        model: String,
        effort: ReasoningEffortConfig,
    ) -> Result<Option<CollaborationMode>, String> {
        let mut document = self.load_taskdriven_document()?;
        match &mut document {
            TaskDrivenConfigDocument::V1(config) => {
                let task = config
                    .task_mut(task_id)
                    .ok_or_else(|| format!("TaskDriven phase {task_id:?} no longer exists"))?;
                task.model = model;
                task.reasoning_effort = effort;
            }
            TaskDrivenConfigDocument::V2(config) => {
                if !config.phases.iter().any(|phase| phase.id == task_id) {
                    return Err(format!("TaskDriven phase {task_id:?} no longer exists"));
                }
                let pin = RoutePin {
                    model,
                    reasoning_effort: effort,
                };
                match config.mode {
                    TaskDrivenMode::Fixed => {
                        config
                            .fixed
                            .as_mut()
                            .ok_or_else(|| "Fixed TaskDriven settings are missing".to_string())?
                            .routes
                            .insert(task_id.to_string(), pin);
                    }
                    TaskDrivenMode::Adaptive => {
                        config
                            .adaptive
                            .as_mut()
                            .ok_or_else(|| "Adaptive TaskDriven settings are missing".to_string())?
                            .pins
                            .insert(task_id.to_string(), pin);
                    }
                }
            }
        }
        self.save_taskdriven_document(document)
    }

    pub(crate) fn set_taskdriven_initial_task(
        &self,
        task_id: String,
    ) -> Result<Option<CollaborationMode>, String> {
        let mut document = self.load_taskdriven_document()?;
        if !taskdriven_phases(&document)
            .iter()
            .any(|phase| phase.id == task_id)
        {
            return Err(format!("TaskDriven phase {task_id:?} no longer exists"));
        }
        match &mut document {
            TaskDrivenConfigDocument::V1(config) => config.initial_task = task_id,
            TaskDrivenConfigDocument::V2(config) => config.initial_phase = task_id,
        }
        self.save_taskdriven_document(document)
    }

    pub(crate) fn add_taskdriven_task(
        &self,
        spec: &str,
    ) -> Result<Option<CollaborationMode>, String> {
        let parts = spec.splitn(3, '|').map(str::trim).collect::<Vec<_>>();
        if parts.len() < 2 || parts[0].is_empty() || parts[1].is_empty() {
            return Err("Use: phase-id | Label | optional description".to_string());
        }
        let mut document = self.load_taskdriven_document()?;
        if taskdriven_phases(&document)
            .iter()
            .any(|phase| phase.id == parts[0])
        {
            return Err(format!("TaskDriven phase {:?} already exists", parts[0]));
        }
        let model = self.current_model().to_string();
        let effort = self.effective_reasoning_effort().unwrap_or_else(|| {
            self.model_catalog
                .try_list_models()
                .unwrap_or_default()
                .into_iter()
                .find(|preset| preset.model == model)
                .map_or(ReasoningEffortConfig::Medium, |preset| {
                    preset.default_reasoning_effort
                })
        });
        match &mut document {
            TaskDrivenConfigDocument::V1(config) => {
                config.tasks.push(TaskDefinition::new(
                    parts[0],
                    parts[1],
                    parts.get(2).copied().unwrap_or_default(),
                    model,
                    effort,
                ));
            }
            TaskDrivenConfigDocument::V2(config) => {
                config.phases.push(PhaseDefinition {
                    id: parts[0].to_string(),
                    label: parts[1].to_string(),
                    description: parts.get(2).copied().unwrap_or_default().to_string(),
                });
                let pin = RoutePin {
                    model,
                    reasoning_effort: effort,
                };
                match config.mode {
                    TaskDrivenMode::Fixed => {
                        config
                            .fixed
                            .as_mut()
                            .ok_or_else(|| "Fixed TaskDriven settings are missing".to_string())?
                            .routes
                            .insert(parts[0].to_string(), pin);
                    }
                    TaskDrivenMode::Adaptive => {
                        config
                            .adaptive
                            .as_mut()
                            .ok_or_else(|| "Adaptive TaskDriven settings are missing".to_string())?
                            .pins
                            .insert(parts[0].to_string(), pin);
                    }
                }
            }
        }
        self.save_taskdriven_document(document)
    }

    pub(crate) fn remove_taskdriven_task(
        &self,
        task_id: &str,
    ) -> Result<Option<CollaborationMode>, String> {
        let mut document = self.load_taskdriven_document()?;
        if taskdriven_phases(&document).len() == 1 {
            return Err("The last TaskDriven phase cannot be removed".to_string());
        }
        match &mut document {
            TaskDrivenConfigDocument::V1(config) => {
                let Some(index) = config.tasks.iter().position(|task| task.id == task_id) else {
                    return Err(format!("TaskDriven phase {task_id:?} no longer exists"));
                };
                config.tasks.remove(index);
                if config.initial_task == task_id {
                    config.initial_task = config.tasks[0].id.clone();
                }
            }
            TaskDrivenConfigDocument::V2(config) => {
                let Some(index) = config.phases.iter().position(|phase| phase.id == task_id) else {
                    return Err(format!("TaskDriven phase {task_id:?} no longer exists"));
                };
                config.phases.remove(index);
                if config.initial_phase == task_id {
                    config.initial_phase = config.phases[0].id.clone();
                }
                match config.mode {
                    TaskDrivenMode::Fixed => {
                        if let Some(fixed) = config.fixed.as_mut() {
                            fixed.routes.remove(task_id);
                        }
                    }
                    TaskDrivenMode::Adaptive => {
                        if let Some(adaptive) = config.adaptive.as_mut() {
                            adaptive.pins.remove(task_id);
                        }
                    }
                }
            }
        }
        self.save_taskdriven_document(document)
    }

    pub(crate) fn set_taskdriven_profile(
        &self,
        profile: String,
    ) -> Result<Option<CollaborationMode>, String> {
        let mut document = self.load_taskdriven_document()?;
        let TaskDrivenConfigDocument::V2(config) = &mut document else {
            return Err("Adaptive profiles require TaskDriven v2".to_string());
        };
        let adaptive = config
            .adaptive
            .as_mut()
            .ok_or_else(|| "TaskDriven is not in adaptive mode".to_string())?;
        adaptive.profile = profile;
        self.save_taskdriven_document(document)
    }

    pub(crate) fn set_taskdriven_use_case(
        &self,
        use_case: String,
    ) -> Result<Option<CollaborationMode>, String> {
        let mut document = self.load_taskdriven_document()?;
        let TaskDrivenConfigDocument::V2(config) = &mut document else {
            return Err("Use cases require TaskDriven v2".to_string());
        };
        let adaptive = config
            .adaptive
            .as_mut()
            .ok_or_else(|| "TaskDriven is not in adaptive mode".to_string())?;
        adaptive.use_case = use_case;
        self.save_taskdriven_document(document)
    }

    pub(crate) fn migrate_taskdriven_to_adaptive(
        &self,
    ) -> Result<Option<CollaborationMode>, String> {
        let document =
            TaskDrivenConfigDocument::migrate_v1_to_adaptive(self.config.codex_home.as_path())
                .map_err(|error| error.to_string())?;
        self.save_taskdriven_document(document)
    }

    fn load_taskdriven_document(&self) -> Result<TaskDrivenConfigDocument, String> {
        TaskDrivenConfigDocument::load(self.config.codex_home.as_path())
            .map_err(|error| error.to_string())
    }

    fn materialize_taskdriven_document(
        &self,
        document: &TaskDrivenConfigDocument,
        presets: &[ModelPreset],
    ) -> Result<TaskDrivenConfig, String> {
        let models = presets
            .iter()
            .map(TaskDrivenModel::from_preset)
            .collect::<Vec<_>>();
        let config = document
            .materialize_for_session(&models)
            .map_err(|error| error.to_string())?;
        Self::validate_taskdriven_catalog(&config, presets)?;
        Ok(config)
    }

    fn save_taskdriven_document(
        &self,
        document: TaskDrivenConfigDocument,
    ) -> Result<Option<CollaborationMode>, String> {
        let presets = self.model_catalog.try_list_models().map_err(|_| {
            "Models are being updated; try saving TaskDriven again in a moment".to_string()
        })?;
        let config = self.materialize_taskdriven_document(&document, &presets)?;
        document
            .save(self.config.codex_home.as_path())
            .map_err(|error| error.to_string())?;
        if !self.taskdriven_selected() {
            return Ok(None);
        }
        let mut mode = self.effective_collaboration_mode();
        mode.settings.model_routing = ModelRouting::TaskDriven;
        mode.settings.developer_instructions = Some(codex_taskdriven::enable_instructions(
            mode.settings.developer_instructions.as_deref(),
            &config,
        ));
        Ok(Some(mode))
    }

    pub(super) fn validate_taskdriven_catalog(
        config: &TaskDrivenConfig,
        presets: &[ModelPreset],
    ) -> Result<(), String> {
        for task in &config.tasks {
            let Some(preset) = presets.iter().find(|preset| preset.model == task.model) else {
                return Err(format!(
                    "Task {} uses model {}, which is not in the live catalog",
                    task.label, task.model
                ));
            };
            let effort_supported = preset.default_reasoning_effort == task.reasoning_effort
                || preset
                    .supported_reasoning_efforts
                    .iter()
                    .any(|option| option.effort == task.reasoning_effort);
            if !effort_supported {
                return Err(format!(
                    "Task {} uses unsupported {}/{}",
                    task.label, task.model, task.reasoning_effort
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn set_taskdriven_selection(&mut self) {
        self.set_model("gpt-5.6-sol");
        self.set_reasoning_effort(Some(ReasoningEffortConfig::XHigh));
        self.current_collaboration_mode.settings.model_routing = ModelRouting::TaskDriven;
        self.config.model_routing = ModelRouting::TaskDriven;
        self.refresh_model_dependent_surfaces();
        self.request_redraw();
    }
}

fn taskdriven_phases(document: &TaskDrivenConfigDocument) -> Vec<PhaseDefinition> {
    match document {
        TaskDrivenConfigDocument::V1(config) => {
            config.tasks.iter().map(PhaseDefinition::from).collect()
        }
        TaskDrivenConfigDocument::V2(config) => config.phases.clone(),
    }
}

fn taskdriven_initial_phase(document: &TaskDrivenConfigDocument) -> &str {
    match document {
        TaskDrivenConfigDocument::V1(config) => &config.initial_task,
        TaskDrivenConfigDocument::V2(config) => &config.initial_phase,
    }
}

fn taskdriven_phase_is_pinned(document: &TaskDrivenConfigDocument, phase_id: &str) -> bool {
    match document {
        TaskDrivenConfigDocument::V1(_) => true,
        TaskDrivenConfigDocument::V2(config) => match config.mode {
            TaskDrivenMode::Fixed => true,
            TaskDrivenMode::Adaptive => config
                .adaptive
                .as_ref()
                .is_some_and(|adaptive| adaptive.pins.contains_key(phase_id)),
        },
    }
}

fn taskdriven_profile_name(config: &TaskDrivenConfigV2) -> String {
    let reference = config
        .adaptive
        .as_ref()
        .map(|adaptive| adaptive.profile.as_str())
        .unwrap_or("unknown");
    codex_taskdriven::builtin_profiles()
        .into_iter()
        .find(|profile| format!("builtin:{}@1", profile.id) == reference)
        .map_or_else(|| reference.to_string(), |profile| profile.name)
}

fn taskdriven_use_case_name(config: &TaskDrivenConfigV2) -> String {
    let id = config
        .adaptive
        .as_ref()
        .map(|adaptive| adaptive.use_case.as_str())
        .unwrap_or("unknown");
    TASKDRIVEN_USE_CASES
        .iter()
        .find(|(candidate, _, _)| *candidate == id)
        .map_or_else(|| id.to_string(), |(_, name, _)| (*name).to_string())
}
