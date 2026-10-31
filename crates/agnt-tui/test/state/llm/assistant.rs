use super::*;

use agnt_core::session::tool_status::ToolStatus;

/// An assistant message is appended to the transcript before the turn
/// completes; turn completion then unlocks input.
#[test]
fn assistant_message_is_appended_to_transcript_before_completed_unlock() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    reduce(
        &mut state,
        SystemEvent::PromptSubmitted {
            text: "ping".to_string(),
        },
        None,
    );
    let _ = state.activate_next_prompt();
    assert!(!state.input_locked);

    // -- Exec
    reduce_llm(&mut state, assistant_message("pong"));

    // -- Check: [User, Assistant] — content entries only, no separators.
    assert!(!state.input_locked);
    assert_eq!(
        state
            .transcript
            .entries()
            .iter()
            .map(entry_text)
            .collect::<Vec<_>>(),
        vec!["ping", "pong"]
    );
    assert!(matches!(
        &state.transcript.entries()[0],
        TranscriptEntry::Message(message) if message.role == MessageRole::User
    ));
    assert!(is_assistant_entry(&state.transcript.entries()[1]));

    // -- Exec: turn completion unlocks via the reducer.
    reduce(
        &mut state,
        agnt_core::protocol::events::SystemEvent::TurnCompleted {
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        },
        None,
    );
    state.finalize_cycle();

    // -- Check
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    Ok(())
}

/// Assistant streaming appends one block, trims whitespace, ignores
/// whitespace-only text, replaces rather than appends on each delta, and
/// resets the stream cursor on a new LLM start.
#[test]
fn assistant_streaming_append_and_replace() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = busy_state_with_clean_transcript();

    // -- Exec & Check: whitespace-only text is a no-op.
    reduce_llm(&mut state, assistant_message(" \n\t\n"));
    assert!(state.transcript.entries().is_empty());

    // -- Exec & Check: text is trimmed and appended as one entry.
    reduce_llm(&mut state, assistant_message("\nline 1\nline 2\n"));
    let assistant_entries: Vec<String> = state
        .transcript
        .entries()
        .iter()
        .filter(|entry| is_assistant_entry(entry))
        .map(entry_text)
        .collect();
    assert_eq!(assistant_entries.len(), 1, "one assistant entry");
    assert!(assistant_entries[0].contains("line 1"));
    assert!(assistant_entries[0].contains("line 2"));

    // -- Exec & Check: a new delta replaces the entry, not appends.
    let mut stream = busy_state_with_clean_transcript();
    reduce_llm(&mut stream, assistant_message("hello"));
    assert!(stream.transcript.assistant_stream_start.is_some());
    let first_start = stream
        .transcript
        .assistant_stream_start
        .ok_or("should have assistant_stream_start")?;
    reduce_llm(
        &mut stream,
        SystemEvent::InferenceChunk {
            delta: " world".to_string(),
            aggregated: "hello world".to_string(),
        },
    );
    assert_eq!(stream.transcript.assistant_stream_start, Some(first_start));
    let stream_entries = stream.transcript.entries();
    let assistant_entries: Vec<_> = stream_entries
        .iter()
        .filter(|entry| is_assistant_entry(entry))
        .collect();
    assert_eq!(assistant_entries.len(), 1);
    assert_eq!(entry_text(assistant_entries[0]), "hello world");
    assert!(
        !stream_entries
            .iter()
            .any(|entry| entry_text(entry) == "hello")
    );

    // -- Exec & Check: InferenceStarted resets the stream cursor.
    reduce_llm(&mut stream, SystemEvent::InferenceStarted);
    assert!(stream.transcript.assistant_stream_start.is_none());

    // -- Exec & Check: a fresh state pushes one assistant entry, no spacer.
    let mut fresh = AppState::default();
    reduce_llm(&mut fresh, assistant_message("hello"));
    assert!(is_assistant_entry(&fresh.transcript.entries()[0]));
    Ok(())
}

/// A dry-run diff the tool entry's unified display already shows is redundant:
/// the dedup reads the last tool entry through the store (typed
/// `ContentKind::Diff`), so the regurgitating assistant message is dropped.
#[test]
fn assistant_diff_regurgitation_is_suppressed() -> Result<()> {
    // -- Setup & Fixtures: a completed edit carrying the diff display.
    let mut state = AppState::default();
    completed_edit_with_diff(&mut state);
    let before = state.transcript.len();

    // -- Exec
    reduce_llm(&mut state, assistant_message(REGURGITATED_DIFF));

    // -- Check: the message adds no entry; the tool entry is the last entry.
    assert_eq!(
        state.transcript.len(),
        before,
        "the regurgitated diff must not be stored again"
    );
    assert!(!state.transcript.entries().iter().any(is_assistant_entry));
    Ok(())
}

/// The check walks the store past non-tool entries: an intervening assistant
/// message no longer blinds it, so a later regurgitation is still suppressed
/// and the intervening message stays.
#[test]
fn assistant_diff_regurgitation_is_suppressed_after_an_intervening_message() -> Result<()> {
    // -- Setup & Fixtures: a completed edit, one unrelated message, then a new
    // stream (InferenceStarted resets the streaming cursor).
    let mut state = AppState::default();
    completed_edit_with_diff(&mut state);
    reduce_llm(&mut state, assistant_message("Applying the change now."));
    reduce_llm(&mut state, SystemEvent::InferenceStarted);
    assert_eq!(
        state.transcript.len(),
        2,
        "the unrelated message precedes the regurgitating stream"
    );

    // -- Exec
    reduce_llm(&mut state, assistant_message(REGURGITATED_DIFF));

    // -- Check: nothing is stored and the intervening message survives.
    assert_eq!(
        state.transcript.len(),
        2,
        "an intervening message must not blind the dedup"
    );
    let texts: Vec<String> = state
        .transcript
        .entries()
        .iter()
        .filter(|entry| is_assistant_entry(entry))
        .map(entry_text)
        .collect();
    assert_eq!(texts, vec!["Applying the change now.".to_string()]);
    Ok(())
}

/// Without a tool display there is nothing to dedup: the message is processed
/// and projected through markdown.
#[test]
fn assistant_diff_regurgitation_is_processed_without_a_tool_display() -> Result<()> {
    // -- Setup & Fixtures
    let mut plain = AppState::default();

    // -- Exec
    reduce_llm(&mut plain, assistant_message(REGURGITATED_DIFF));

    // -- Check
    assert!(!plain.transcript.entries().is_empty());
    assert!(plain.transcript.entries().iter().any(is_assistant_entry));
    Ok(())
}

// ---------------------------------------------------------------------------
// Test Support
// ---------------------------------------------------------------------------

/// The assistant message that regurgitates the fixture edit's diff.
const REGURGITATED_DIFF: &str =
    "Dry-run diff:\n```diff\n--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n```";

/// Fold a start + completion pair for `edit sample.txt` carrying the diff
/// display the dedup compares against.
fn completed_edit_with_diff(state: &mut AppState) {
    let edit_arguments = r#"{"path":"sample.txt"}"#;
    let edit_call = || {
        agnt_core::protocol::events::ToolCall::new(
            agnt_core::transcript::ir::ToolName("edit".to_string()),
            "closure",
            edit_arguments,
            CallLine::from_json_summary(edit_arguments),
        )
    };
    reduce(
        state,
        agnt_core::protocol::events::SystemEvent::ToolStarted { call: edit_call() },
        None,
    );
    reduce(
        state,
        agnt_core::protocol::events::SystemEvent::ToolCompleted {
            call: edit_call(),
            outcome: agnt_core::protocol::events::ToolOutcome {
                success: true,
                status: ToolStatus::Success,
                result: "{}".to_string(),
                display: Some(ToolDisplay {
                    title: Some("edit sample.txt".to_string()),
                    sections: vec![ToolDisplaySection {
                        label: Some("sample.txt".to_string()),
                        kind: agnt_core::transcript::ir::ContentKind::Diff {
                            language: "diff".to_string(),
                            path: "test.rs".to_string(),
                        },
                        content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                            .to_string(),
                        stats: None,
                    }],
                }),
                error_kind: None,
                message: None,
            },
        },
        None,
    );
}
