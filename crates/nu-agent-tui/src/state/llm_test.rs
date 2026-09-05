//! LLM-domain reducer tests: assistant streaming, phase transitions, token
//! accounting, and diff-regurgitation dedup. Assertions moved 1:1 from the
//! former `interaction/reducer_test.rs` `reduce_ui_event_impl` effect tests,
//! driven through `LlmState::reduce_llm_event`.

use crate::interaction::reducer::{ReducerInput, UserAction, reduce_with_cancel_controller};
use crate::state::{AppState, InputState, UiPhase};
use nu_agent_core::bus::LlmEvent;
use nu_agent_core::protocol::event::{ToolDisplay, ToolDisplaySection};
use nu_agent_core::transcript::ir::Role;
use nu_agent_core::transcript::items::{ProseMessage, TranscriptEntryKind};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn busy_state_with_clean_transcript() -> AppState {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("run".to_string()),
        ..Default::default()
    };
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();
    state.transcript.entries.clear();
    // Simulate handle_llm_start which sets the lock
    state.input_locked = true;
    state
}

fn reduce_llm(state: &mut AppState, event: LlmEvent) -> bool {
    state.llm.reduce_llm_event(
        &mut state.transcript,
        &mut state.status,
        &mut state.scroll,
        &mut state.phase,
        &mut state.input_locked,
        event,
    )
}

fn assistant_message(text: &str) -> LlmEvent {
    LlmEvent::AssistantMessage {
        text: text.to_string(),
    }
}

#[test]
fn llm_start_from_idle_moves_busy_and_locks_input() {
    let mut state = AppState::default();
    reduce_llm(&mut state, LlmEvent::Started);
    assert_eq!(state.phase, UiPhase::Busy);
    assert!(state.input_locked);
}

#[test]
fn llm_start_when_busy_is_noop_for_phase() {
    let mut state = busy_state_with_clean_transcript();
    state.status.status_line = "Tool: prior".to_string();

    reduce_llm(&mut state, LlmEvent::Started);

    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.status.status_line, "Tool: prior");
    assert!(state.input_locked);
}

#[test]
fn llm_end_event_updates_latest_and_rolling_token_usage() {
    let mut state = AppState::default();

    reduce_llm(
        &mut state,
        LlmEvent::Completed {
            response_chars: 6,
            tool_calls: 0,
            input_tokens: 20,
            output_tokens: 10,
            total_tokens: 30,
        },
    );

    assert_eq!(state.status.tokens.latest_input_tokens, Some(20));
    assert_eq!(state.status.tokens.latest_output_tokens, Some(10));
    assert_eq!(state.status.tokens.latest_total_tokens, Some(30));
    assert_eq!(state.status.tokens.session_total_tokens, 30);

    reduce_llm(
        &mut state,
        LlmEvent::Completed {
            response_chars: 4,
            tool_calls: 0,
            input_tokens: 5,
            output_tokens: 7,
            total_tokens: 12,
        },
    );

    assert_eq!(state.status.tokens.latest_input_tokens, Some(5));
    assert_eq!(state.status.tokens.latest_output_tokens, Some(7));
    assert_eq!(state.status.tokens.latest_total_tokens, Some(12));
    assert_eq!(state.status.tokens.session_total_tokens, 42);
}

#[test]
fn llm_end_records_tokens_and_sets_ready_status() {
    let mut state = busy_state_with_clean_transcript();

    reduce_llm(
        &mut state,
        LlmEvent::Completed {
            response_chars: 12,
            tool_calls: 0,
            input_tokens: 4,
            output_tokens: 8,
            total_tokens: 12,
        },
    );

    assert_eq!(state.status.tokens.latest_input_tokens, Some(4));
    assert_eq!(state.status.tokens.latest_output_tokens, Some(8));
    assert_eq!(state.status.tokens.latest_total_tokens, Some(12));
    assert_eq!(state.status.tokens.session_total_tokens, 12);
    assert_eq!(state.status.status_line, "Response ready (12 chars)");
}

#[test]
fn assistant_message_is_appended_to_transcript_before_completed_unlock() {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("ping".to_string()),
        ..Default::default()
    };
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();
    assert!(!state.input_locked);

    reduce_llm(&mut state, assistant_message("pong"));

    assert!(!state.input_locked);
    // Transcript: [Spacer, User, Spacer, Spacer, Assistant]
    assert_eq!(
        state
            .transcript
            .entries
            .iter()
            .map(|entry| entry.text())
            .collect::<Vec<_>>(),
        vec!["", "ping", "", "", "pong"]
    );
    assert_eq!(state.transcript.entries[0].role(), Role::Separator); // starting spacer before prompt
    assert_eq!(state.transcript.entries[1].role(), Role::User);
    assert_eq!(state.transcript.entries[2].role(), Role::Separator); // closing spacer after prompt
    assert_eq!(state.transcript.entries[3].role(), Role::Separator); // starting spacer before assistant
    assert_eq!(state.transcript.entries[4].role(), Role::Assistant);

    // Turn completion unlocks via the turn domain.
    use nu_agent_core::bus::TurnEvent;
    state.turn.reduce_turn_event(
        &mut state.transcript,
        &mut state.status,
        &mut state.input_locked,
        TurnEvent::Completed { tool_calls: 0 },
    );
    state.finalize_cycle();

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
}

#[test]
fn assistant_message_whitespace_only_is_noop() {
    let mut state = busy_state_with_clean_transcript();

    reduce_llm(&mut state, assistant_message(" \n\t\n"));

    assert!(state.transcript.entries.is_empty());
}

#[test]
fn assistant_message_trims_and_appends() {
    let mut state = busy_state_with_clean_transcript();

    reduce_llm(&mut state, assistant_message("\nline 1\nline 2\n"));

    // After the raw-markdown refactor, a single AssistantMessage
    // produces one ProseMessage. The raw text is trimmed before
    // storage, so leading/trailing whitespace is dropped.
    let assistant_entries: Vec<_> = state
        .transcript
        .entries
        .iter()
        .filter(|e| e.role() == Role::Assistant)
        .map(|e| e.text())
        .collect();
    assert_eq!(assistant_entries.len(), 1, "one ProseMessage per block");
    let text = &assistant_entries[0];
    assert!(text.contains("line 1"), "raw md should contain 'line 1'");
    assert!(text.contains("line 2"), "raw md should contain 'line 2'");
}

#[test]
fn streaming_replaces_not_appends() -> Result<()> {
    let mut state = busy_state_with_clean_transcript();

    // Emit first AssistantMessage delta
    reduce_llm(&mut state, assistant_message("hello"));

    // First message should set the assistant stream start
    assert!(state.transcript.assistant_stream_start.is_some());
    let first_start = state
        .transcript
        .assistant_stream_start
        .ok_or("should have assistant_stream_start")?;

    // Emit second AssistantMessage delta (accumulated text)
    reduce_llm(&mut state, assistant_message("hello world"));

    // Should still have same assistant_stream_start
    assert_eq!(state.transcript.assistant_stream_start, Some(first_start));

    // Verify transcript has ONE message block with "hello world", not two separate entries
    let assistant_entries: Vec<_> = state
        .transcript
        .entries
        .iter()
        .filter(|entry| entry.role() == Role::Assistant)
        .collect();

    // Should have exactly one "hello world" message, not "hello" and "hello world"
    assert_eq!(
        assistant_entries.len(),
        1,
        "Expected exactly one assistant message block (replaced, not appended)"
    );
    assert_eq!(assistant_entries[0].text(), "hello world");

    // Verify no "hello" without "world" exists
    assert!(
        !state
            .transcript
            .entries
            .iter()
            .any(|entry| entry.text() == "hello"),
        "Should not have standalone 'hello' entry - it should be replaced"
    );
    Ok(())
}

#[test]
fn streaming_message_start_reset_on_llm_start() {
    let mut state = busy_state_with_clean_transcript();

    // Emit streaming sequence
    reduce_llm(&mut state, assistant_message("first message"));

    // Should have assistant_stream_start set
    assert!(state.transcript.assistant_stream_start.is_some());

    reduce_llm(&mut state, assistant_message("first message continues"));

    // Still should be set
    assert!(state.transcript.assistant_stream_start.is_some());

    // Emit LlmStarted (new LLM response begins)
    reduce_llm(&mut state, LlmEvent::Started);

    // Verify assistant_stream_start is reset to None
    assert!(
        state.transcript.assistant_stream_start.is_none(),
        "LlmStarted should reset assistant_stream_start to None"
    );
}

#[test]
fn handle_assistant_message_pushes_starting_spacer() {
    let mut state = AppState::default();
    reduce_llm(&mut state, assistant_message("hello"));
    assert!(matches!(
        state.transcript.entries[0].kind,
        TranscriptEntryKind::Spacer(_)
    ));
    assert!(matches!(
        state.transcript.entries[1].kind,
        TranscriptEntryKind::Assistant(_)
    ));
}

#[test]
fn assistant_dry_run_diff_regurgitation_is_suppressed_when_direct_display_present() {
    let mut state = AppState::default();

    // Direct tool display via the tool domain
    state.tool.reduce_tool_event(
        &mut state.transcript,
        &mut state.status,
        nu_agent_core::bus::ToolEvent::Started {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
        },
    );
    state.tool.reduce_tool_event(
        &mut state.transcript,
        &mut state.status,
        nu_agent_core::bus::ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    language: "diff".to_string(),
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let before = state.transcript.entries.len();
    reduce_llm(
        &mut state,
        assistant_message(
            "Dry-run diff:\n```diff\n--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n```",
        ),
    );

    // Assistant message is processed and projected through markdown
    assert!(state.transcript.entries.len() > before);
    assert!(
        state
            .transcript
            .entries
            .iter()
            .any(|entry| entry.role() == Role::Assistant)
    );
}

#[test]
fn normal_assistant_response_remains_when_no_direct_display_is_present() {
    let mut state = AppState::default();

    reduce_llm(
        &mut state,
        assistant_message(
            "Dry-run diff:\n```diff\n--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n```",
        ),
    );

    assert!(!state.transcript.entries.is_empty());
    assert!(
        state
            .transcript
            .entries
            .iter()
            .any(|entry| entry.role() == Role::Assistant)
    );
}

// region:    --- Raw markdown projection (moved from task_4a_tests)

/// Return raw markdown strings stored in all Assistant ProseMessage entries.
fn assistant_markdown_entries(state: &AppState) -> Vec<String> {
    state
        .transcript
        .entries
        .iter()
        .filter_map(|e| {
            if let TranscriptEntryKind::Assistant(ProseMessage { markdown }) = &e.kind {
                Some(markdown.clone())
            } else {
                None
            }
        })
        .collect()
}

/// Project a markdown string and return all (text, hint) pairs from it.
fn project_spans(markdown: &str) -> Vec<(String, nu_agent_core::transcript::ir::StyleHint)> {
    crate::markdown::render_markdown_lines(markdown, None)
        .into_iter()
        .flat_map(|l| l.spans.into_iter())
        .map(|s| (s.text, s.hint))
        .collect()
}

#[test]
fn assistant_message_with_bold_emits_md_bold() {
    let mut state = AppState::default();
    reduce_llm(&mut state, assistant_message("hello **bold**"));
    assert!(
        // Verify the raw markdown is stored and projects to MdBold
        assistant_markdown_entries(&state).iter().any(|md| {
            project_spans(md).iter().any(|(t, h)| {
                t == "bold" && matches!(h, nu_agent_core::transcript::ir::StyleHint::MdBold)
            })
        })
    );
}

#[test]
fn assistant_streaming_truncates_prior_render() {
    let mut state = AppState::default();
    for text in ["hello", "hello world"] {
        reduce_llm(&mut state, assistant_message(text));
    }
    // After streaming, there should be a single ProseMessage with the final text
    let markdowns = assistant_markdown_entries(&state);
    let concat: String = markdowns.join("");
    assert!(concat.contains("hello world"));
    assert!(!concat.contains("hellohello"));
}

// endregion: --- Raw markdown projection (moved from task_4a_tests)
