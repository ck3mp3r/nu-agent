use crate::state::*;
use nu_agent_core::protocol::event::PermissionDecision;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn permission_prompt_open_sets_presence() {
    let mut state = AppState::default();
    state.permission.open_prompt(PermissionPrompt {
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

    assert!(state.permission.has_prompt());
}

#[test]
fn permission_prompt_open_does_not_scroll() {
    let mut state = AppState::default();
    state
        .transcript
        .push_transcript_line(TranscriptRole::User, "msg1".to_string());
    state
        .transcript
        .push_transcript_line(TranscriptRole::Assistant, "msg2".to_string());
    state
        .transcript
        .push_transcript_line(TranscriptRole::Tool, "tool1".to_string());
    state.scroll.scroll_transcript_to_top();
    assert!(!state.scroll.following_tail);

    state.permission.open_prompt(PermissionPrompt {
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

    assert!(!state.scroll.following_tail);
}

#[test]
fn submit_permission_decision_enqueues_submission_and_closes_prompt() -> Result<()> {
    let mut state = AppState::default();
    state.permission.open_prompt(PermissionPrompt {
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

    assert!(
        state
            .permission
            .submit_decision(PermissionDecision::AllowAlways)
    );
    assert!(!state.permission.has_prompt());

    let submission = state
        .permission
        .take_next_submission()
        .ok_or("should have queued permission submission")?;
    assert_eq!(submission.request_id, "ask-0000000000000002");
    assert_eq!(submission.matched_rule_identity, "nested:nu.command:*");
    assert_eq!(submission.decision, PermissionDecision::AllowAlways);
    Ok(())
}
