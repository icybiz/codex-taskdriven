//! Selection and persistence of the work-phase routing mode.

use super::*;
use codex_protocol::config_types::ModelRouting;

impl App {
    pub(super) async fn select_taskdriven(&mut self, app_server: &mut AppServerSession) {
        if !self
            .chat_widget
            .config_ref()
            .features
            .enabled(Feature::AutonomousWorkPhase)
            || !self
                .chat_widget
                .config_ref()
                .features
                .enabled(Feature::StepModelSwitching)
        {
            self.chat_widget.add_error_message(
                "Automatic model selection is unavailable in this session.".to_string(),
            );
            return;
        }
        let mut mode = self
            .chat_widget
            .effective_collaboration_mode()
            .with_updates(
                Some("gpt-5.6-sol".to_string()),
                Some(Some(ReasoningEffortConfig::XHigh)),
                /*developer_instructions*/ None,
            );
        mode.settings.model_routing = ModelRouting::TaskDriven;
        if let Some(thread_id) = self.active_thread_id {
            match app_server
                .thread_settings_update(codex_app_server_protocol::ThreadSettingsUpdateParams {
                    thread_id: thread_id.to_string(),
                    model: Some(mode.model().to_string()),
                    effort: mode.reasoning_effort(),
                    collaboration_mode: Some(mode),
                    ..Default::default()
                })
                .await
            {
                Ok(true) => {}
                Ok(false) => {
                    self.chat_widget.add_error_message(
                        "This server does not support changing model selection.".to_string(),
                    );
                    return;
                }
                Err(error) => {
                    self.chat_widget.add_error_message(format!(
                        "Failed to enable automatic model selection: {error}"
                    ));
                    return;
                }
            }
        }
        self.chat_widget.set_taskdriven_selection();
        self.config.model_routing = ModelRouting::TaskDriven;
        self.config.model_reasoning_effort = Some(ReasoningEffortConfig::XHigh);
        let edits = crate::config_update::build_model_routing_selection_edits(
            "gpt-5.6-sol",
            Some(ReasoningEffortConfig::XHigh),
            ModelRouting::TaskDriven,
        );
        match crate::config_update::write_config_batch(app_server.request_handle(), edits).await {
            Ok(_) => self.chat_widget.add_info_message(
                "Automatic model selection enabled for future turns: Sol xhigh for writing, investigation and testing; Astra max for implementation.".to_string(), /*hint*/ None,
            ),
            Err(error) => self.chat_widget.add_error_message(format!("Automatic model selection is active, but saving its default failed: {error}")),
        }
    }
}
