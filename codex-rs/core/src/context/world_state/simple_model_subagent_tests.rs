use super::*;
use pretty_assertions::assert_eq;

#[test]
fn simple_model_subagent_instructions_are_added_once_and_bounded() {
    let section = SimpleModelSubagentState::new(true);
    let fragment = section
        .render_diff(PreviousSectionState::Absent)
        .expect("enabled simple subagent should receive escalation guidance");

    assert_eq!(
        fragment.markers(),
        ("<simple_model_subagent>", "</simple_model_subagent>")
    );
    let body = fragment.body();
    assert!(body.contains("CODEX_SIMPLE_MODEL_ESCALATION: yes"));
    assert!(body.contains("CODEX_SIMPLE_MODEL_ESCALATION: no"));
    assert!(body.len() < 1_000);
    assert!(
        section
            .render_diff(PreviousSectionState::Known(&true))
            .is_none()
    );
}

#[test]
fn simple_model_subagent_instructions_are_absent_when_disabled() {
    assert!(
        SimpleModelSubagentState::new(false)
            .render_diff(PreviousSectionState::Absent)
            .is_none()
    );
}
