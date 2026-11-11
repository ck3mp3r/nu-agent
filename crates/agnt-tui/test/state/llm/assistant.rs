use super::*;

use agnt_runtime::session::tool_status::ToolStatus;

/// An assistant message is appended to the transcript before the turn
/// completes; turn completion then unlocks input.
#[test]
fn assistant_message_is_appended_to_transcript_before_completed_unlock() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    reduce_llm(&mut state, prompt_record("ping", None, None, None));
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
    reduce_llm(&mut state, turn_completed(0));
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
        agnt_runtime::protocol::events::NarrationEvent::InferenceChunk {
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
    reduce_llm(&mut stream, inference_started());
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
    reduce_llm(&mut state, inference_started());
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

/// Prose-only chunks carry no diff signature, so the fold truncates and
/// re-pushes one streamed entry: a longer delta replaces it, and folding the
/// same text twice still leaves exactly one entry at the stream start.
#[test]
fn test_llm_assistant_message_prose_chunk_pushes_one_entry() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = busy_state_with_clean_transcript();

    // -- Exec & Check: the first prose delta pushes one entry after the start.
    reduce_llm(&mut state, assistant_message("Here is the plan."));
    let start = state
        .transcript
        .assistant_stream_start
        .ok_or("should have assistant_stream_start")?;
    assert_eq!(state.transcript.len(), start + 1);

    // -- Exec & Check: a longer prose delta replaces the entry, not appends.
    reduce_llm(
        &mut state,
        assistant_message("Here is the plan.\nStep one."),
    );
    assert_eq!(state.transcript.assistant_stream_start, Some(start));
    assert_eq!(state.transcript.len(), start + 1);
    assert_eq!(
        assistant_markdown_entries(&state),
        vec!["Here is the plan.\nStep one.".to_string()]
    );

    // -- Exec & Check: the same aggregated text folded again keeps one entry.
    reduce_llm(
        &mut state,
        assistant_message("Here is the plan.\nStep one."),
    );
    assert_eq!(state.transcript.len(), start + 1);
    Ok(())
}

/// A diff marker wrapped in markdown emphasis is prose to the raw pre-scan:
/// its projection starts with `--- ` but no raw line does, so the fold pushes
/// the chunk (the accepted divergence, documented at the pre-scan).
#[test]
fn test_llm_assistant_message_emphasis_wrapped_diff_marker_pushes() -> Result<()> {
    // -- Setup & Fixtures: a completed edit whose diff is the dedup target.
    let mut state = AppState::default();
    completed_edit_with_diff(&mut state);
    let before = state.transcript.len();
    let wrapped = "**--- x**";
    let projected_line = project_spans(wrapped)
        .into_iter()
        .map(|(text, _)| text)
        .collect::<String>();
    assert!(
        projected_line.trim_start().starts_with("--- "),
        "the projection must surface a diff prefix the raw line does not carry"
    );

    // -- Exec
    reduce_llm(&mut state, assistant_message(wrapped));

    // -- Check: the streamed entry is pushed.
    assert_eq!(state.transcript.len(), before + 1);
    assert_eq!(
        assistant_markdown_entries(&state),
        vec![wrapped.to_string()]
    );
    Ok(())
}

/// A raw diff signature the tool display does not contain is not redundant, so
/// the signature branch keeps HEAD's push.
#[test]
fn test_llm_assistant_message_unrelated_diff_signature_pushes() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    completed_edit_with_diff(&mut state);
    let before = state.transcript.len();
    let text = "```diff\n--- a/other.txt\n+++ b/other.txt\n@@ -1 +1 @@\n-old\n+other\n```";

    // -- Exec
    reduce_llm(&mut state, assistant_message(text));

    // -- Check
    assert_eq!(state.transcript.len(), before + 1);
    assert_eq!(assistant_markdown_entries(&state), vec![text.to_string()]);
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
        agnt_runtime::protocol::events::ToolCall::new(
            agnt_render::transcript::ir::ToolName("edit".to_string()),
            agnt_runtime::tools::handler::ToolSource::Closure,
            edit_arguments,
        )
    };
    reduce_llm(state, tool_started(edit_call()));
    reduce_llm(
        state,
        tool_completed(
            edit_call(),
            agnt_runtime::protocol::events::ToolOutcome {
                status: ToolStatus::Success,
                payload: None,
                text: r#"{"path":"sample.txt","diff":"--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"}"#
                    .to_string(),
                message: None,
                kind: None,
            },
        ),
    );
}
