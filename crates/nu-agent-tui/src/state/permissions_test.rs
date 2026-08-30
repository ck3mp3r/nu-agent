use crate::state::*;
use nu_agent_core::protocol::event::PermissionDecision;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn permission_prompt_open_sets_required_status_and_presence() {
    let mut state = AppState::default();
    state.open_permission_prompt(PermissionPrompt {
        request_id: "ask-0000000000000001".to_string(),
        matched_rule_identity: "nested:nu.command:*".to_string(),
        tool: "nu".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        scope: "nested".to_string(),
        pattern: "*".to_string(),
        target_field: Some("command".to_string()),
        summary: "→ {\"command\":\"echo hi\"}".to_string(),
    });

    assert!(state.has_permission_prompt());
    assert_eq!(state.status_line, "Permission required");
}

#[test]
fn permission_prompt_open_scrolls_to_bottom() {
    let mut state = AppState::default();
    state.push_transcript_line(TranscriptRole::User, "msg1".to_string());
    state.push_transcript_line(TranscriptRole::Assistant, "msg2".to_string());
    state.push_transcript_line(TranscriptRole::Tool, "tool1".to_string());
    state.scroll_transcript_to_top();
    assert!(!state.transcript_following_tail);

    state.open_permission_prompt(PermissionPrompt {
        request_id: "ask-001".to_string(),
        matched_rule_identity: "rule".to_string(),
        tool: "edit".to_string(),
        source: "builtin".to_string(),
        mode: None,
        scope: "global".to_string(),
        pattern: "*".to_string(),
        target_field: None,
        summary: "edit foo.rs".to_string(),
    });

    assert!(state.transcript_following_tail);
}

#[test]
fn submit_permission_decision_enqueues_submission_and_closes_prompt() -> Result<()> {
    let mut state = AppState::default();
    state.open_permission_prompt(PermissionPrompt {
        request_id: "ask-0000000000000002".to_string(),
        matched_rule_identity: "nested:nu.command:*".to_string(),
        tool: "nu".to_string(),
        source: "closure".to_string(),
        mode: None,
        scope: "nested".to_string(),
        pattern: "*".to_string(),
        target_field: Some("command".to_string()),
        summary: "summary".to_string(),
    });

    assert!(state.submit_permission_decision(PermissionDecision::AllowAlways));
    assert!(!state.has_permission_prompt());

    let submission = state
        .take_next_permission_decision_submission()
        .ok_or("should have queued permission submission")?;
    assert_eq!(submission.request_id, "ask-0000000000000002");
    assert_eq!(submission.matched_rule_identity, "nested:nu.command:*");
    assert_eq!(submission.decision, PermissionDecision::AllowAlways);
    Ok(())
}
