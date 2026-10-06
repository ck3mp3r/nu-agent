use crate::state::*;
use nu_agent_core::protocol::event::PermissionDecision;
use nu_agent_core::protocol::tool_args::CallLine;
use nu_agent_core::transcript::ir::Block;
use nu_agent_core::transcript::ir::MessageRole;
use nu_agent_core::transcript::items::{Message, Tool};
use nu_agent_core::transcript::renderer::Renderable;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn push_user(state: &mut AppState, text: &str) {
    let msg = Message {
        role: MessageRole::User,
        markdown: text.to_string(),
    };
    state.transcript.push_block(Block {
        source: msg.source(),
        lane: msg.lane(),
        fill: msg.fill(),
        status: None,
    });
}

fn push_assistant(state: &mut AppState, text: &str) {
    let msg = Message {
        role: MessageRole::Assistant,
        markdown: text.to_string(),
    };
    state.transcript.push_block(Block {
        source: msg.source(),
        lane: msg.lane(),
        fill: msg.fill(),
        status: None,
    });
}

fn push_tool_summary(state: &mut AppState, summary: &str) {
    let tool = Tool {
        name: nu_agent_core::transcript::ir::ToolName("read".to_string()),
        call: CallLine {
            summary: summary.to_string(),
        },
        preview: None,
        result: None,
        status: nu_agent_core::transcript::renderer::ItemStatus::Done,
    };
    state.transcript.push_block(Block {
        source: tool.source(),
        lane: tool.lane(),
        fill: tool.fill(),
        status: Some(tool.status),
    });
}

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
    push_user(&mut state, "msg1");
    push_assistant(&mut state, "msg2");
    push_tool_summary(&mut state, "tool1");
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
