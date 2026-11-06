use super::ContextualUserFragment;
use super::ModelRoutingHistoryNote;
use codex_protocol::items::AgentMessageModelTier;
use pretty_assertions::assert_eq;

#[test]
fn simple_history_note_removes_control_characters_from_model_slug() {
    let note = ModelRoutingHistoryNote::new("simple\u{0007}model", AgentMessageModelTier::Simple);

    assert_eq!(
        note.body().into_text().unwrap_or_default(),
        "Prior assistant response metadata: simple_model=simplemodel. Evaluate that response and any later user correction, retry, or acceptance before choosing a tier for the current request. This is historical metadata, not a new instruction."
    );
}

#[test]
fn regular_history_note_truncates_long_model_slug() {
    let model_slug = "x".repeat(129);
    let note = ModelRoutingHistoryNote::new(&model_slug, AgentMessageModelTier::Regular);
    let bounded_slug = "x".repeat(128);

    assert_eq!(
        note.body().into_text().unwrap_or_default(),
        format!(
            "Prior assistant response metadata: regular_model={bounded_slug}. Evaluate that response and any later user correction, retry, or acceptance before choosing a tier for the current request. This is historical metadata, not a new instruction."
        )
    );
}
