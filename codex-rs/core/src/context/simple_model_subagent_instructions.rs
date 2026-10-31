use super::ContextualUserFragment;
use codex_protocol::models::ContentItemKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SimpleModelSubagentInstructions;

impl ContextualUserFragment for SimpleModelSubagentInstructions {
    fn content_kind(&self) -> ContentItemKind {
        ContentItemKind("simple_model_subagent.instructions".to_string())
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
        ("<simple_model_subagent>", "</simple_model_subagent>")
    }

    fn body(&self) -> String {
        "Complete the assigned task and send the parent your best response. End with exactly one assessment marker: `CODEX_SIMPLE_MODEL_ESCALATION: no` if you answered adequately, or `CODEX_SIMPLE_MODEL_ESCALATION: yes` only if your answer is materially inadequate and the regular model could materially improve it. Do not ask the parent to escalate for polish alone.".to_string()
    }
}
