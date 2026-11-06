use super::*;
use pretty_assertions::assert_eq;

#[test]
fn simple_model_subagent_instructions_are_added_once_and_bounded() {
    let section = SimpleModelSubagentState::new(true);
    let (snapshot, mut updates) = section.render_diff(PreviousSectionState::Absent);
    assert_eq!(snapshot, Some(true));
    let fragment = updates
        .pop()
        .expect("enabled simple subagent should receive escalation guidance")
        .content
        .into_fragment()
        .expect("expected fragment update");

    assert_eq!(
        fragment.markers(),
        ("<simple_model_subagent>", "</simple_model_subagent>")
    );
    let body = fragment.body().into_text().unwrap_or_default();
    assert!(body.contains("CODEX_SIMPLE_MODEL_ESCALATION: yes"));
    assert!(body.contains("CODEX_SIMPLE_MODEL_ESCALATION: no"));
    assert!(body.len() < 1_000);
    let (snapshot, updates) = section.render_diff(PreviousSectionState::Known(&true));
    assert_eq!(snapshot, Some(true));
    assert!(updates.is_empty());
}

#[test]
fn simple_model_subagent_instructions_are_absent_when_disabled() {
    let (snapshot, updates) =
        SimpleModelSubagentState::new(false).render_diff(PreviousSectionState::Absent);
    assert_eq!(snapshot, Some(false));
    assert!(updates.is_empty());
}
