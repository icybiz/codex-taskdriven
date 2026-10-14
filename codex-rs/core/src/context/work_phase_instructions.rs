use super::ContextualUserFragment;
use super::world_state::PreviousSectionState;
use super::world_state::SectionTransition;
use super::world_state::WorldStateHash;
use super::world_state::WorldStateSection;
use codex_protocol::models::ContentItemKind;
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
        (Some(current), fragment)
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

    fn body(&self) -> String {
        if !self.enabled {
            return "Autonomous work-phase routing is disabled. All previously provided autonomous work-phase instructions no longer apply.".to_string();
        }
        "These instructions replace all previously provided autonomous work-phase instructions. Autonomous work-phase routing is enabled by the user. Select the phase yourself whenever the nature of the work changes, including repeatedly within one user request. Do not ask the user to request or approve routine model switches.\n\n\
         Use writing for prose, explanations, documentation and final answers; investigation for research, code reading, bug discovery and diagnosis; verification for running existing tests and interpreting their results. These phases use gpt-5.6-sol with xhigh reasoning. Use implementation for designing or changing program code, fixing bugs, implementing features, refactoring, and creating or modifying automated tests. This phase uses gpt-6-astra with max reasoning.\n\n\
         Before performing work belonging to a different model, call set_work_phase directly as the only tool call in that response, then continue after its result. A successful switch takes effect on the next model request in this same thread; finish this response without additional text or tools. Do not write implementation code before handing off to Astra. After implementation, switch back before ordinary testing or prose. If a test exposes another required code change, switch to implementation again before fixing it. Running tools are not restarted by a switch. If switching is rejected, report the reason and do not silently perform that phase using the wrong model. This routing does not grant additional permissions or change approval requirements."
            .to_string()
    }
}
