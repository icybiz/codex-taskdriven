use super::*;
use codex_protocol::config_types::ModelRouting;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn taskdriven_selection_updates_thread_and_persisted_default_and_can_be_disabled()
-> Result<()> {
    Box::pin(async {
        let mut app = make_test_app().await;
        for feature in [Feature::StepModelSwitching, Feature::AutonomousWorkPhase] {
            app.config.features.enable(feature)?;
            app.chat_widget
                .set_feature_enabled(feature, /*enabled*/ true);
        }
        let mut app_server = crate::start_embedded_app_server_for_picker(&app.config).await?;
        let started = app_server.start_thread(&app.config).await?;
        let thread_id = started.session.thread_id;
        app.enqueue_primary_thread_session(started.session, started.turns)
            .await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        app.handle_event(&mut tui, &mut app_server, AppEvent::SelectTaskDriven)
            .await?;
        let updated = next_thread_settings_updated(&mut app_server, thread_id).await;
        assert_eq!(
            (
                updated.thread_settings.model,
                updated.thread_settings.effort,
                updated
                    .thread_settings
                    .collaboration_mode
                    .settings
                    .model_routing
            ),
            (
                "gpt-5.6-sol".to_string(),
                Some(ReasoningEffortConfig::XHigh),
                ModelRouting::TaskDriven
            ),
        );
        assert!(app.chat_widget.taskdriven_selected());
        let config_path = app.config.codex_home.join("config.toml");
        let persisted: toml::Value = toml::from_str(&std::fs::read_to_string(&config_path)?)?;
        assert_eq!(persisted["model_routing"].as_str(), Some("taskDriven"));
        assert_eq!(persisted["model"].as_str(), Some("gpt-5.6-sol"));
        assert_eq!(persisted["model_reasoning_effort"].as_str(), Some("xhigh"));
        let new_thread = app_server.start_thread(&app.config).await?;
        assert_eq!(
            (
                new_thread.session.model,
                new_thread.session.reasoning_effort,
                new_thread.session.model_routing
            ),
            (
                "gpt-5.6-sol".to_string(),
                Some(ReasoningEffortConfig::XHigh),
                ModelRouting::TaskDriven
            ),
        );

        // Selecting that same concrete model must still exit automatic routing.
        app.handle_event(
            &mut tui,
            &mut app_server,
            AppEvent::UpdateModel("gpt-5.6-sol".to_string()),
        )
        .await?;
        let updated = next_thread_settings_updated(&mut app_server, thread_id).await;
        assert_eq!(
            updated
                .thread_settings
                .collaboration_mode
                .settings
                .model_routing,
            ModelRouting::Fixed
        );
        assert!(!app.chat_widget.taskdriven_selected());
        app.handle_event(
            &mut tui,
            &mut app_server,
            AppEvent::PersistModelSelection {
                model: "gpt-5.6-sol".to_string(),
                effort: Some(ReasoningEffortConfig::XHigh),
            },
        )
        .await?;
        let persisted: toml::Value = toml::from_str(&std::fs::read_to_string(config_path)?)?;
        assert_eq!(persisted["model_routing"].as_str(), Some("fixed"));
        app_server.shutdown().await?;
        Ok(())
    })
    .await
}
