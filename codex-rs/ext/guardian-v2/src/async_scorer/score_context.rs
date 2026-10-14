//! Keeps asynchronous scores attached to the model and classifier that produced them.

use codex_extension_api::ExtensionData;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::security_risk::SecurityRiskScore;

use super::authorization::ScoreAuthorization;
use super::config::GuardianV2Config;

#[derive(Default)]
pub(super) struct ScoreContext {
    configuration: Option<(ModelInfo, GuardianV2Config)>,
    generation: u64,
    pub(super) authorization: Option<ScoreAuthorization>,
}

impl ScoreContext {
    /// Called before any asynchronous work at tool start. Returning to an earlier
    /// model creates a new generation too; its old in-flight results stay stale.
    #[expect(
        clippy::expect_used,
        reason = "reusing a generation at counter exhaustion could authorize a stale score"
    )]
    pub(super) fn begin(&mut self, model: &ModelInfo, config: &GuardianV2Config) -> u64 {
        if !self.matches(model, config) {
            self.configuration = Some((model.clone(), config.clone()));
            self.generation = self
                .generation
                .checked_add(1)
                .expect("score generation overflow");
            self.authorization = None;
        }
        self.generation
    }

    /// Called while holding the shared score lock, so the score and its validity
    /// metadata become visible atomically to fast approval decisions.
    pub(super) fn publish(
        &mut self,
        generation: u64,
        authorization: ScoreAuthorization,
        store: &ExtensionData,
        score: &SecurityRiskScore,
    ) -> bool {
        if !self.is_current(generation) {
            return false;
        }
        let accepted = store.insert_if(score.clone(), |previous| {
            previous.is_none_or(|previous| previous.sampled_at < score.sampled_at)
        });
        if accepted {
            self.authorization = Some(authorization);
        }
        accepted
    }

    pub(super) fn matches(&self, model: &ModelInfo, config: &GuardianV2Config) -> bool {
        self.configuration
            .as_ref()
            .is_some_and(|(captured_model, captured_config)| {
                captured_model == model && captured_config == config
            })
    }

    pub(super) fn is_current(&self, generation: u64) -> bool {
        self.configuration.is_some() && self.generation == generation
    }
}
