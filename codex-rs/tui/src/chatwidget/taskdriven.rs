use super::*;
use codex_taskdriven::TaskDefinition;
use codex_taskdriven::TaskDrivenConfig;

const TASKDRIVEN_SELECTION_VIEW_ID: &str = "taskdriven-selection";

impl ChatWidget {
    pub(super) fn taskdriven_selected(&self) -> bool {
        codex_taskdriven::instructions_are_enabled(
            self.effective_collaboration_mode()
                .settings
                .developer_instructions
                .as_deref(),
        )
    }

    pub(super) fn taskdriven_selection_item(
        &self,
        presets: &[ModelPreset],
    ) -> Option<SelectionItem> {
        if !self.config.features.enabled(Feature::StepModelSwitching) {
            return None;
        }
        let config = match TaskDrivenConfig::load(self.config.codex_home.as_path()) {
            Ok(config) => config,
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
        let initial = config.initial()?;
        let catalog_note = Self::validate_taskdriven_catalog(&config, presets).err();
        Some(SelectionItem {
            name: "TaskDriven".to_string(),
            description: Some(catalog_note.unwrap_or_else(|| {
                format!(
                    "Route {} editable tasks by phase · starts with {}/{}",
                    config.tasks.len(),
                    initial.model,
                    initial.reasoning_effort
                )
            })),
            is_current: self.taskdriven_selected(),
            actions: vec![Box::new(|tx| tx.send(AppEvent::OpenTaskDrivenPopup))],
            dismiss_on_select: true,
            ..Default::default()
        })
    }

    pub(crate) fn open_taskdriven_popup(&mut self) {
        let config = match self.load_taskdriven_config() {
            Ok(config) => config,
            Err(error) => {
                self.add_error_message(error);
                return;
            }
        };
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
        for task in &config.tasks {
            let task_id = task.id.clone();
            items.push(SelectionItem {
                name: format!("{} · {}", task.label, task.id),
                description: Some(format!(
                    "{}/{}{}",
                    task.model,
                    task.reasoning_effort,
                    if task.description.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", task.description)
                    }
                )),
                is_current: task.id == config.initial_task,
                actions: vec![Box::new(move |tx| {
                    tx.send(AppEvent::OpenTaskDrivenModelPopup {
                        task_id: task_id.clone(),
                    });
                })],
                dismiss_on_select: true,
                ..Default::default()
            });
        }
        items.extend([
            SelectionItem {
                name: "Choose initial task…".to_string(),
                description: Some("Select the route used when TaskDriven is enabled.".to_string()),
                actions: vec![Box::new(|tx| {
                    tx.send(AppEvent::OpenTaskDrivenInitialTaskPopup);
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Add task…".to_string(),
                description: Some("Create a task, then choose its model and effort.".to_string()),
                actions: vec![Box::new(|tx| {
                    tx.send(AppEvent::OpenTaskDrivenAddPrompt);
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Remove task…".to_string(),
                description: Some("Delete a configured task.".to_string()),
                is_disabled: config.tasks.len() == 1,
                disabled_reason: (config.tasks.len() == 1)
                    .then_some("The last task cannot be removed".to_string()),
                actions: vec![Box::new(|tx| {
                    tx.send(AppEvent::OpenTaskDrivenRemovePopup);
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
        ]);
        let mut header = ColumnRenderable::new();
        header.push(Line::from("TaskDriven routing".bold()));
        header.push(Line::from("Stored in $CODEX_HOME/taskdriven.toml".dim()));
        self.bottom_pane.show_selection_view(SelectionViewParams {
            view_id: Some(TASKDRIVEN_SELECTION_VIEW_ID),
            header: Box::new(header),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    pub(crate) fn open_taskdriven_model_popup(&mut self, task_id: String) {
        let config = match self.load_taskdriven_config() {
            Ok(config) => config,
            Err(error) => {
                self.add_error_message(error);
                return;
            }
        };
        let Some(task) = config.task(&task_id) else {
            self.add_error_message(format!("TaskDriven task {task_id:?} no longer exists"));
            return;
        };
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
                    is_current: preset.model == task.model,
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
            title: Some(format!("Model for {}", task.label)),
            subtitle: Some("Choose from the live model catalog.".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            is_searchable: true,
            search_placeholder: Some("Type to search models".to_string()),
            ..Default::default()
        });
    }

    pub(crate) fn open_taskdriven_effort_popup(&mut self, task_id: String, preset: ModelPreset) {
        let config = match self.load_taskdriven_config() {
            Ok(config) => config,
            Err(error) => {
                self.add_error_message(error);
                return;
            }
        };
        let Some(task) = config.task(&task_id) else {
            self.add_error_message(format!("TaskDriven task {task_id:?} no longer exists"));
            return;
        };
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
                let selected = task.model == model && task.reasoning_effort == effort;
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
            title: Some(format!("Reasoning for {}", task.label)),
            subtitle: Some(format!("Model: {}", preset.model)),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    pub(crate) fn open_taskdriven_initial_task_popup(&mut self) {
        let config = match self.load_taskdriven_config() {
            Ok(config) => config,
            Err(error) => {
                self.add_error_message(error);
                return;
            }
        };
        let items = config
            .tasks
            .iter()
            .map(|task| {
                let task_id = task.id.clone();
                SelectionItem {
                    name: task.label.clone(),
                    description: Some(format!("{}/{}", task.model, task.reasoning_effort)),
                    is_current: task.id == config.initial_task,
                    actions: vec![Box::new(move |tx| {
                        tx.send(AppEvent::SetTaskDrivenInitialTask(task_id.clone()));
                    })],
                    dismiss_on_select: true,
                    ..Default::default()
                }
            })
            .collect();
        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Initial TaskDriven task".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    pub(crate) fn open_taskdriven_add_prompt(&mut self) {
        let tx = self.app_event_tx.clone();
        let view = CustomPromptView::new(
            "Add TaskDriven task".to_string(),
            "task-id | Label | Description".to_string(),
            String::new(),
            Some("The new task initially uses the active model and effort.".to_string()),
            Box::new(move |spec| tx.send(AppEvent::AddTaskDrivenTask(spec))),
        );
        self.bottom_pane.show_text_prompt(view);
    }

    pub(crate) fn open_taskdriven_remove_popup(&mut self) {
        let config = match self.load_taskdriven_config() {
            Ok(config) => config,
            Err(error) => {
                self.add_error_message(error);
                return;
            }
        };
        let items = config
            .tasks
            .iter()
            .map(|task| {
                let task_id = task.id.clone();
                SelectionItem {
                    name: task.label.clone(),
                    description: Some(task.id.clone()),
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
            title: Some("Remove TaskDriven task".to_string()),
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
        let mut config = self.load_taskdriven_config()?;
        if enabled {
            let presets = self.model_catalog.try_list_models().map_err(|_| {
                "Models are being updated; try enabling TaskDriven again in a moment".to_string()
            })?;
            Self::validate_taskdriven_catalog(&config, &presets)?;
        }
        config.enabled = enabled;
        config
            .save(self.config.codex_home.as_path())
            .map_err(|error| error.to_string())?;
        let mut mode = self.effective_collaboration_mode();
        if enabled {
            let initial = config.initial().ok_or_else(|| {
                "TaskDriven initial_task no longer names a configured task".to_string()
            })?;
            mode.settings.model = initial.model.clone();
            mode.settings.reasoning_effort = Some(initial.reasoning_effort.clone());
            mode.settings.developer_instructions = Some(codex_taskdriven::enable_instructions(
                mode.settings.developer_instructions.as_deref(),
                &config,
            ));
        } else {
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
        let mut config = self.load_taskdriven_config()?;
        let task = config
            .task_mut(task_id)
            .ok_or_else(|| format!("TaskDriven task {task_id:?} no longer exists"))?;
        task.model = model;
        task.reasoning_effort = effort;
        self.save_taskdriven_config(config)
    }

    pub(crate) fn set_taskdriven_initial_task(
        &self,
        task_id: String,
    ) -> Result<Option<CollaborationMode>, String> {
        let mut config = self.load_taskdriven_config()?;
        if config.task(&task_id).is_none() {
            return Err(format!("TaskDriven task {task_id:?} no longer exists"));
        }
        config.initial_task = task_id;
        self.save_taskdriven_config(config)
    }

    pub(crate) fn add_taskdriven_task(
        &self,
        spec: &str,
    ) -> Result<Option<CollaborationMode>, String> {
        let parts = spec.splitn(3, '|').map(str::trim).collect::<Vec<_>>();
        if parts.len() < 2 || parts[0].is_empty() || parts[1].is_empty() {
            return Err("Use: task-id | Label | optional description".to_string());
        }
        let mut config = self.load_taskdriven_config()?;
        if config.task(parts[0]).is_some() {
            return Err(format!("TaskDriven task {:?} already exists", parts[0]));
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
        config.tasks.push(TaskDefinition::new(
            parts[0],
            parts[1],
            parts.get(2).copied().unwrap_or_default(),
            model,
            effort,
        ));
        self.save_taskdriven_config(config)
    }

    pub(crate) fn remove_taskdriven_task(
        &self,
        task_id: &str,
    ) -> Result<Option<CollaborationMode>, String> {
        let mut config = self.load_taskdriven_config()?;
        if config.tasks.len() == 1 {
            return Err("The last TaskDriven task cannot be removed".to_string());
        }
        let Some(index) = config.tasks.iter().position(|task| task.id == task_id) else {
            return Err(format!("TaskDriven task {task_id:?} no longer exists"));
        };
        config.tasks.remove(index);
        if config.initial_task == task_id {
            config.initial_task = config.tasks[0].id.clone();
        }
        self.save_taskdriven_config(config)
    }

    fn load_taskdriven_config(&self) -> Result<TaskDrivenConfig, String> {
        TaskDrivenConfig::load(self.config.codex_home.as_path()).map_err(|error| error.to_string())
    }

    fn save_taskdriven_config(
        &self,
        config: TaskDrivenConfig,
    ) -> Result<Option<CollaborationMode>, String> {
        config
            .save(self.config.codex_home.as_path())
            .map_err(|error| error.to_string())?;
        if !self.taskdriven_selected() {
            return Ok(None);
        }
        let mut mode = self.effective_collaboration_mode();
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
}
