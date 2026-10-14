//! Selection policy kept separate from the concrete inference model.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

/// Only TaskDriven enables automatic work-phase transitions. Older settings
/// without a routing policy retain ordinary fixed-model behavior.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, TS,
)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ModelRouting {
    #[default]
    Fixed,
    TaskDriven,
}

impl ModelRouting {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::TaskDriven => "taskDriven",
        }
    }
}

#[cfg(test)]
#[path = "model_routing_tests.rs"]
mod tests;
