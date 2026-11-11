use crate::state::*;
use agnt_render::transcript::entry::{MessageEntry, ToolEntry, ToolKind, TranscriptEntry};
use agnt_render::transcript::ir::{MessageRole, ToolName};
use agnt_runtime::protocol::event::PermissionDecision;
use agnt_runtime::protocol::events::ToolCall;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn push_user(state: &mut AppState, text: &str) {
    let entry = TranscriptEntry::Message(MessageEntry {
        role: MessageRole::User,
        markdown: text.to_string(),
        timestamp_millis: None,
        model: None,
        agent: None,
    });
    state.transcript.push_entry(entry);
}

fn push_assistant(state: &mut AppState, text: &str) {
    let entry = TranscriptEntry::Message(MessageEntry {
        role: MessageRole::Assistant,
        markdown: text.to_string(),
        timestamp_millis: None,
        model: None,
        agent: None,
    });
    state.transcript.push_entry(entry);
}

fn push_tool_entry(state: &mut AppState, arguments: &str) {
    let call = ToolCall::new(
        ToolName("read".to_string()),
        agnt_runtime::tools::handler::ToolSource::Unknown,
        arguments,
    );
    let entry = TranscriptEntry::Tool(ToolEntry {
        kind: ToolKind::from_name(&call.name),
        call,
        preview: None,
        result: None,
        status: agnt_render::transcript::renderer::ItemStatus::Done,
    });
    state.transcript.push_entry(entry);
}

#[test]
fn permission_prompt_open_sets_presence_without_scrolling() {
    // -- Setup & Fixtures: a transcript scrolled away from the tail.
    let mut state = AppState::default();
    push_user(&mut state, "msg1");
    push_assistant(&mut state, "msg2");
    push_tool_entry(&mut state, "tool1");
    state.scroll.scroll_transcript_to_top();
    assert!(!state.scroll.following_tail);

    // -- Exec: a request opens its card.
    state.permission.open_prompt("ask-0000000000000001");

    // -- Check
    assert!(state.permission.has_prompt());
    assert!(
        !state.scroll.following_tail,
        "opening a prompt must not scroll the transcript"
    );

    // -- Exec: a global builtin prompt.
    let mut state = AppState::default();

    // -- Exec
    state.permission.open_prompt("ask-0000000000000001");

    // -- Check
    assert!(state.permission.has_prompt());
}

#[test]
fn submit_permission_decision_enqueues_submission_and_closes_prompt() -> Result<()> {
    let mut state = AppState::default();
    state.permission.open_prompt("ask-0000000000000002");

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
    assert_eq!(submission.decision, PermissionDecision::AllowAlways);
    Ok(())
}
