use super::*;

use crate::state::StatusMessageKind;
use agnt_core::conversation::turn::executor::TURN_STOPPED_NOTICE;

// ---------------------------------------------------------------------------
// LlmEvent::Stopped — hook-stop notice (Fix 2)
// ---------------------------------------------------------------------------

/// `LlmEvent::Stopped` APPENDS the reason as a new transcript entry without
/// truncating the streamed assistant block (Fix 2 of the repetition stop UX):
/// the streamed repeated text stays visible, the reason is added after it,
/// and `assistant_stream_start` resets so nothing later truncates the block.
#[test]
fn stopped_event_appends_reason_without_truncating_stream() -> Result<()> {
    // -- Setup & Fixtures: stream an assistant block (2 dedup steps).
    let mut state = busy_state_with_clean_transcript();
    reduce_llm(&mut state, assistant_message("repeated text"));
    reduce_llm(&mut state, assistant_message("repeated text continues"));
    let streamed_start = state
        .transcript
        .assistant_stream_start
        .ok_or("stream should have set assistant_stream_start")?;
    assert!(
        state
            .transcript
            .blocks()
            .iter()
            .any(|b| is_assistant_block(b)
                && b.source.plain_text().contains("repeated text continues")),
        "the streamed block must exist before the stop"
    );

    // -- Exec
    let changed = reduce_llm(
        &mut state,
        LlmEvent::Stopped {
            reason: "Output repetition stopped: the run was stopped.".to_string(),
        },
    );

    // -- Check
    assert!(changed, "Stopped must mark the TUI changed");
    // The streamed block is NOT truncated: the streamed text is still there.
    assert!(
        state
            .transcript
            .blocks()
            .iter()
            .any(|b| is_assistant_block(b)
                && b.source.plain_text().contains("repeated text continues")),
        "the streamed block must survive the stop notice"
    );
    // The reason is appended as a NEW entry after the streamed block.
    let last = state
        .transcript
        .blocks()
        .last()
        .ok_or("should have transcript blocks")?;
    let reason_appended = last
        .source
        .plain_text()
        .contains("Output repetition stopped");
    assert!(
        reason_appended,
        "the stop reason must be appended as a new entry, got {last:?}"
    );
    // The streaming cursor resets.
    assert!(
        state.transcript.assistant_stream_start.is_none(),
        "Stopped must reset assistant_stream_start"
    );
    // The start index is BEFORE the appended reason (stream untouched).
    let _ = streamed_start;

    // -- Setup & Fixtures: an empty reason discards the provisional block.
    let mut state = busy_state_with_clean_transcript();
    reduce_llm(&mut state, assistant_message("streamed block"));
    assert!(state.transcript.assistant_stream_start.is_some());

    // -- Exec
    let changed = reduce_llm(
        &mut state,
        LlmEvent::Stopped {
            reason: "  ".to_string(),
        },
    );

    // -- Check: nothing renders and the provisional block is discarded.
    assert!(!changed, "an empty reason renders nothing");
    assert!(!state.transcript.blocks().iter().any(is_assistant_block));
    assert!(state.transcript.assistant_stream_start.is_none());

    // -- Setup & Fixtures: truncation back to the start index.
    let mut truncating = busy_state_with_clean_transcript();
    reduce_llm(&mut truncating, assistant_message("provisional text"));
    let start = truncating
        .transcript
        .assistant_stream_start
        .ok_or("stream should have set assistant_stream_start")?;
    assert!(
        truncating
            .transcript
            .blocks()
            .iter()
            .any(|b| is_assistant_block(b) && b.source.plain_text().contains("provisional text"))
    );

    // -- Exec
    let changed = reduce_llm(
        &mut truncating,
        LlmEvent::Stopped {
            reason: String::new(),
        },
    );

    // -- Check
    assert!(!changed, "a silent discard renders nothing");
    assert_eq!(truncating.transcript.len(), start);
    assert!(
        !truncating
            .transcript
            .blocks()
            .iter()
            .any(is_assistant_block)
    );
    assert!(truncating.transcript.assistant_stream_start.is_none());

    // -- Setup & Fixtures: an empty transcript.
    let mut empty = busy_state_with_clean_transcript();
    assert!(empty.transcript.blocks().is_empty());

    // -- Exec
    let changed = reduce_llm(
        &mut empty,
        LlmEvent::Stopped {
            reason: String::new(),
        },
    );

    // -- Check: no spacer or notice is pushed.
    assert!(!changed, "a silent discard renders nothing");
    assert!(empty.transcript.blocks().is_empty());
    Ok(())
}

/// `LlmEvent::Stopped` carrying `TURN_STOPPED_NOTICE` renders a status-bar
/// marker instead of a transcript block: the streamed blocks stay, the
/// streaming cursor resets, and the status message holds the marker with
/// `Notice` kind.
#[test]
fn stopped_event_with_turn_stopped_notice_sets_status_marker() -> Result<()> {
    // -- Setup & Fixtures: stream an assistant block.
    let mut state = busy_state_with_clean_transcript();
    reduce_llm(&mut state, assistant_message("partial answer"));
    let blocks_before = state.transcript.len();
    assert!(blocks_before > 0, "the streamed block must exist");

    // -- Exec
    let changed = reduce_llm(
        &mut state,
        LlmEvent::Stopped {
            reason: TURN_STOPPED_NOTICE.to_string(),
        },
    );

    // -- Check: the marker is set and no transcript block is appended.
    assert!(changed, "the stop marker must mark the TUI changed");
    assert_eq!(state.status.message.status_line(), TURN_STOPPED_NOTICE);
    assert_eq!(state.status.message.kind(), StatusMessageKind::Notice);
    assert_eq!(
        state.transcript.len(),
        blocks_before,
        "the stop marker must not append a transcript block"
    );
    assert!(
        state
            .transcript
            .blocks()
            .iter()
            .any(|b| is_assistant_block(b) && b.source.plain_text().contains("partial answer")),
        "the streamed block must survive the stop marker"
    );
    assert!(
        state.transcript.assistant_stream_start.is_none(),
        "the stop marker must reset assistant_stream_start"
    );
    Ok(())
}

/// The stop marker has no TTL, so the next turn start retires it. Turn start
/// clears only `Notice`: a `Warning` and a `Neutral` message keep their own
/// expiry.
#[test]
fn turn_stopped_notice_marker_clears_on_next_turn_start() -> Result<()> {
    // -- Setup & Fixtures: a stop marker left by a cancelled turn.
    let mut state = busy_state_with_clean_transcript();
    reduce_llm(
        &mut state,
        LlmEvent::Stopped {
            reason: TURN_STOPPED_NOTICE.to_string(),
        },
    );
    assert_eq!(state.status.message.kind(), StatusMessageKind::Notice);

    // -- Exec: the next turn starts streaming.
    reduce_llm(&mut state, LlmEvent::Started);

    // -- Check: the marker is gone.
    assert!(
        state.status.message.status_line().is_empty(),
        "turn start must clear the stop marker"
    );
    assert_eq!(state.status.message.kind(), StatusMessageKind::Neutral);
    assert!(!state.status.message.is_pending());

    // -- Setup & Fixtures: a Warning and a Neutral message.
    let mut warning = busy_state_with_clean_transcript();
    warning
        .status
        .message
        .set_warning("Output repetition stopped: …");
    let mut neutral = busy_state_with_clean_transcript();
    neutral.status.message.set_message("Tool: prior");

    // -- Exec
    reduce_llm(&mut warning, LlmEvent::Started);
    reduce_llm(&mut neutral, LlmEvent::Started);

    // -- Check: turn start leaves the other kinds alone.
    assert_eq!(
        warning.status.message.status_line(),
        "Output repetition stopped: …"
    );
    assert_eq!(warning.status.message.kind(), StatusMessageKind::Warning);
    assert_eq!(neutral.status.message.status_line(), "Tool: prior");
    Ok(())
}
