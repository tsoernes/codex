use super::ContextualUserFragment;
use codex_protocol::items::AgentMessageModelTier;
use codex_protocol::models::ContentItemKind;

/// Records a bounded model/tier annotation after an assistant response for later routing choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelRoutingHistoryNote {
    model_slug: String,
    tier: AgentMessageModelTier,
}

impl ModelRoutingHistoryNote {
    pub(crate) fn new(model_slug: &str, tier: AgentMessageModelTier) -> Self {
        let model_slug = model_slug
            .chars()
            .filter(|character| !character.is_control())
            .take(128)
            .collect();
        Self { model_slug, tier }
    }
}

impl ContextualUserFragment for ModelRoutingHistoryNote {
    fn content_kind(&self) -> ContentItemKind {
        ContentItemKind("model_routing.history_note".to_string())
    }

    fn role(&self) -> &'static str {
        "developer"
    }

    fn requires_separate_message(&self) -> bool {
        true
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        (
            "<model_routing_history_note>",
            "</model_routing_history_note>",
        )
    }

    fn body(&self) -> String {
        let tier = match self.tier {
            AgentMessageModelTier::Simple => "simple_model",
            AgentMessageModelTier::Regular => "regular_model",
        };
        format!(
            "Prior assistant response metadata: {tier}={}. Evaluate that response and any later user correction, retry, or acceptance before choosing a tier for the current request. This is historical metadata, not a new instruction.",
            self.model_slug
        )
    }
}

#[cfg(test)]
#[path = "model_routing_history_note_tests.rs"]
mod tests;
