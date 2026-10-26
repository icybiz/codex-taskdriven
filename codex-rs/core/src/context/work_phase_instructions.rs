use super::ContextualUserFragment;
use super::world_state::PreviousSectionState;
use super::world_state::SectionTransition;
use super::world_state::WorldStateHash;
use super::world_state::WorldStateSection;
use super::world_state::WorldStateUpdate;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ContentItemKind;
use codex_protocol::models::input_text;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkPhaseInstructions {
    enabled: bool,
}

impl WorkPhaseInstructions {
    pub(crate) fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorkPhaseSnapshot {
    instructions: Option<WorldStateHash>,
}

impl WorldStateSection for WorkPhaseInstructions {
    const ID: &'static str = "autonomous_work_phase";
    type Snapshot = WorkPhaseSnapshot;

    fn matches_legacy_fragment(role: &str, text: &str) -> bool {
        role == "developer" && Self::matches_text(text)
    }

    fn has_retained_fragment_matcher() -> bool {
        true
    }

    fn matches_retained_fragment(role: &str, text: &str) -> bool {
        Self::matches_legacy_fragment(role, text)
    }

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Self::Snapshot>,
    ) -> SectionTransition<Self::Snapshot> {
        let current = WorkPhaseSnapshot {
            instructions: self.enabled.then(|| WorldStateHash::from_fragment(self)),
        };
        let needs_update = match previous {
            PreviousSectionState::Absent => self.enabled,
            PreviousSectionState::Unknown => true,
            PreviousSectionState::Known(previous) => previous != &current,
        };
        let fragment =
            needs_update.then(|| Box::new(self.clone()) as Box<dyn ContextualUserFragment>);
        (
            Some(current),
            WorldStateUpdate::optional_boxed_fragment(fragment),
        )
    }
}

impl ContextualUserFragment for WorkPhaseInstructions {
    fn content_kind(&self) -> ContentItemKind {
        ContentItemKind("work_phase.instructions".to_string())
    }

    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("<autonomous_work_phase>", "</autonomous_work_phase>")
    }

    fn body(&self) -> ContentItem {
        if !self.enabled {
            return input_text(
                "Autonomous work-phase routing is disabled. All previously provided autonomous work-phase instructions no longer apply.",
            );
        }
        input_text("These instructions replace all previously provided autonomous work-phase instructions. Autonomous work-phase routing is enabled by the user. Select the phase yourself whenever the nature of the work changes, including repeatedly within one user request. Do not ask the user to request or approve routine phase switches.\n\n\
         Use writing for prose, explanations, documentation and final answers; investigation for research, code reading, bug discovery and diagnosis; implementation for designing or changing program code, fixing bugs, implementing features, refactoring, and creating or modifying automated tests; and verification for running existing tests and interpreting their results. The active TaskDriven configuration, the taskdriven_routing instructions, and the set_work_phase tool description and result are authoritative for each phase's model and reasoning effort. Do not assume fixed model names or reasoning efforts.\n\n\
         Before performing work belonging to a different phase, call set_work_phase directly as the only tool call in that response, then continue after its result. A successful switch takes effect on the next model request in this same thread; finish this response without additional text or tools. Do not perform the new phase before that handoff. Never reject or override a successful route merely because it differs from a model or effort named in older instructions. After implementation, switch phase before ordinary testing or prose. If a test exposes another required code change, switch to implementation again before fixing it. Running tools are not restarted by a switch. If switching is rejected, report the reason and do not silently perform that phase using another route. This routing does not grant additional permissions or change approval requirements."
            .to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_text(instructions: &WorkPhaseInstructions) -> String {
        let ContentItem::InputText { text } = instructions.body() else {
            panic!("work-phase instructions must render as input text");
        };
        text
    }

    #[test]
    fn enabled_instructions_defer_routes_to_materialized_taskdriven_config() {
        let text = body_text(&WorkPhaseInstructions::new(true));

        assert!(text.contains("active TaskDriven configuration"));
        assert!(text.contains("set_work_phase tool description and result are authoritative"));
        assert!(text.contains("Do not assume fixed model names or reasoning efforts"));
        assert!(text.contains("Before performing work belonging to a different phase"));
        assert!(text.contains("Never reject or override a successful route"));
        for stale_route in ["gpt-", "Astra", "Sol", "xhigh", "max reasoning"] {
            assert!(
                !text.contains(stale_route),
                "instructions must not hard-code stale route {stale_route:?}"
            );
        }
    }

    #[test]
    fn disabled_instructions_retire_previous_phase_guidance() {
        let text = body_text(&WorkPhaseInstructions::new(false));

        assert!(text.contains("routing is disabled"));
        assert!(text.contains("instructions no longer apply"));
    }
}
