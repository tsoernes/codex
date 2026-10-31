use super::PreviousSectionState;
use super::WorldStateSection;
use crate::context::ContextualUserFragment;
use crate::context::SimpleModelSubagentInstructions;

/// Escalation guidance for a subagent running on the configured simpler model.
#[derive(Clone, Debug)]
pub(crate) struct SimpleModelSubagentState {
    enabled: bool,
}

impl SimpleModelSubagentState {
    pub(crate) fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

impl WorldStateSection for SimpleModelSubagentState {
    const ID: &'static str = "simple_model_subagent";
    type Snapshot = bool;

    fn snapshot(&self) -> Self::Snapshot {
        self.enabled
    }

    fn matches_current_legacy_fragment(&self, role: &str, text: &str) -> bool {
        self.enabled && role == "developer" && text == SimpleModelSubagentInstructions.body()
    }

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Self::Snapshot>,
    ) -> Option<Box<dyn ContextualUserFragment>> {
        match (self.enabled, previous) {
            (true, PreviousSectionState::Known(true)) | (false, _) => None,
            (true, PreviousSectionState::Known(false) | PreviousSectionState::Absent) => {
                Some(Box::new(SimpleModelSubagentInstructions))
            }
            (true, PreviousSectionState::Unknown) => {
                Some(Box::new(SimpleModelSubagentInstructions))
            }
        }
    }
}

#[cfg(test)]
#[path = "simple_model_subagent_tests.rs"]
mod tests;
