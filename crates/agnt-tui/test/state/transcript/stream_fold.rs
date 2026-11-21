use super::*;

/// Folding a streamed chunk replaces the entry's markdown and leaves the entry
/// where it is: the entry count and the entry's identity survive, so no push
/// and no eviction churn runs per chunk.
#[test]
fn test_transcript_store_fold_stream_markdown_at_replaces_the_entry_in_place() -> Result<()> {
    // -- Setup & Fixtures: an indexed store with a user turn and a streamed
    // assistant entry.
    let width = 60;
    let mut state = AppState::default();
    push_user(&mut state, "run it");
    push_assistant(&mut state, "first");
    state.transcript.rebuild_height_index(width);
    let index = state.transcript.len() - 1;
    let len_before = state.transcript.len();

    // -- Exec
    state
        .transcript
        .fold_stream_markdown_at(index, "first\nsecond".to_string());

    // -- Check: same entry slot, same entry count, the new text.
    assert_eq!(
        state.transcript.len(),
        len_before,
        "a fold must not push a second entry"
    );
    assert_eq!(
        state.transcript.entries()[index],
        assistant_entry_with("first\nsecond"),
        "the fold must rewrite the entry's markdown in place"
    );
    Ok(())
}

/// The fold defers the measurement: the cached rows stay stale until the paint
/// path flushes them, and the flush re-measures exactly the folded entry.
#[test]
fn test_transcript_store_fold_stream_markdown_at_defers_the_measure_to_the_frame() -> Result<()> {
    // -- Setup & Fixtures
    let width = 52;
    let mut state = AppState::default();
    push_user(&mut state, "run it");
    push_assistant(&mut state, "first");
    state.transcript.rebuild_height_index(width);
    let index = state.transcript.len() - 1;
    let measured_before = state.transcript.remeasured_total();

    // -- Exec: fold ten chunks, each taller than the last.
    for line in 0..10 {
        state
            .transcript
            .fold_stream_markdown_at(index, (0..=line).map(prose_line).collect());
    }

    // -- Check: the folds measured nothing.
    assert_eq!(
        state.transcript.remeasured_total(),
        measured_before,
        "a fold must defer the measure to the paint"
    );

    // -- Exec: the paint reads the index.
    state.transcript.ensure_height_index(width);

    // -- Check: one measure for the whole burst, and the geometry matches a
    // wholesale rebuild of the folded entries.
    assert_eq!(
        state.transcript.remeasured_total(),
        measured_before + 1,
        "a burst of folds must cost one measure per frame"
    );
    assert_geometry_matches_wholesale(&state, width)?;

    // -- Exec & Check: a frame with nothing to flush measures nothing.
    state.transcript.ensure_height_index(width);
    assert_eq!(
        state.transcript.remeasured_total(),
        measured_before + 1,
        "an idle frame must re-measure nothing"
    );
    Ok(())
}

/// A fold whose index holds no assistant message — the streamed cursor was
/// lost — falls back to the pre-fold shape: the tail from the index is dropped
/// and one fresh assistant entry is pushed.
#[test]
fn test_transcript_store_fold_stream_markdown_at_pushes_when_no_message_is_there() -> Result<()> {
    // -- Setup & Fixtures: an entry count that equals the cursor, so the index
    // addresses nothing yet.
    let mut state = AppState::default();
    push_user(&mut state, "run it");
    let index = state.transcript.len();

    // -- Exec
    state
        .transcript
        .fold_stream_markdown_at(index, "streamed".to_string());

    // -- Check: one assistant entry at the cursor.
    assert_eq!(state.transcript.len(), index + 1);
    assert_eq!(
        state.transcript.entries()[index],
        assistant_entry_with("streamed"),
        "the fallback must push an assistant entry at the cursor"
    );

    // -- Setup & Fixtures: a cursor pointing at a tool entry, with an entry
    // after it.
    let mut shadowed = AppState::default();
    push_user(&mut shadowed, "run it");
    shadowed.transcript.push_entry(simple_tool_entry());
    push_assistant(&mut shadowed, "stale tail");
    let tool_index = shadowed.transcript.len() - 2;

    // -- Exec
    shadowed
        .transcript
        .fold_stream_markdown_at(tool_index, "streamed".to_string());

    // -- Check: the tail from the cursor is dropped and the entry is pushed.
    assert_eq!(shadowed.transcript.len(), tool_index + 1);
    assert_eq!(
        shadowed.transcript.entries()[tool_index],
        assistant_entry_with("streamed"),
        "the fallback must replace the tail from the cursor"
    );
    Ok(())
}

/// Cap eviction shifts the cursor and the deferred measure together, so the
/// next chunk lands on the streamed entry and the frame measures that entry
/// once — no stale index, no duplicate entry.
#[test]
fn test_transcript_store_fold_stream_markdown_at_follows_cap_eviction() -> Result<()> {
    // -- Setup & Fixtures: a full transcript whose last entry is the streamed
    // one, with a fold pending over a live index.
    let width = 64;
    let mut state = AppState::default();
    for row in 0..MAX_TRANSCRIPT_ENTRIES {
        push_user(&mut state, &format!("row {row}"));
    }
    state.transcript.rebuild_height_index(width);
    let cursor = state.transcript.len() - 1;
    state.transcript.assistant_stream_start = Some(cursor);
    state
        .transcript
        .fold_stream_markdown_at(cursor, "streamed one".to_string());

    // -- Exec: one more push overflows the cap and evicts the oldest entry.
    push_user(&mut state, "next turn");

    // -- Check: the cursor followed the eviction.
    assert_eq!(
        state.transcript.len(),
        MAX_TRANSCRIPT_ENTRIES,
        "the cap holds"
    );
    assert_eq!(
        state.transcript.assistant_stream_start,
        Some(cursor - 1),
        "eviction must shift the streaming cursor"
    );

    // -- Exec: the next chunk folds at the shifted cursor. The push left an
    // entry pending, so the frame drains it first; the fold defers its own mark,
    // and the check below counts only what the next frame measures.
    let shifted = state
        .transcript
        .assistant_stream_start
        .ok_or("the cursor must have shifted, not dropped")?;
    state.transcript.ensure_height_index(width);
    state
        .transcript
        .fold_stream_markdown_at(shifted, "streamed one\ntwo".to_string());
    let measured_before = state.transcript.remeasured_total();
    state.transcript.ensure_height_index(width);

    // -- Check: the streamed entry carries the full text, the store holds one
    // such entry, and its rows match a wholesale rebuild.
    assert_eq!(
        state.transcript.entries()[shifted],
        assistant_entry_with("streamed one\ntwo"),
        "the fold must land on the shifted streamed entry"
    );
    let streamed = state
        .transcript
        .entries()
        .iter()
        .filter(|entry| {
            matches!(
                entry,
                TranscriptEntry::Message(message)
                    if message.role == MessageRole::Assistant && message.markdown.contains("streamed")
            )
        })
        .count();
    assert_eq!(
        streamed, 1,
        "the store must hold exactly one streamed entry"
    );
    assert_eq!(
        state.transcript.remeasured_total(),
        measured_before + 1,
        "the frame must measure the shifted entry once"
    );
    assert_geometry_matches_wholesale(&state, width)?;
    Ok(())
}

/// The diff gate's answer belongs to one paint window: it is answered once,
/// reused for every later fold in that window, and open again after the frame
/// that read the index.
#[test]
fn test_transcript_store_diff_gate_answers_once_per_paint_window() -> Result<()> {
    // -- Setup & Fixtures: a frame has read the index, so a window is open.
    let width = 60;
    let mut state = AppState::default();
    push_assistant(&mut state, "--- a/x");
    state.transcript.rebuild_height_index(width);
    state.transcript.ensure_height_index(width);
    assert_eq!(
        state.transcript.diff_gate(),
        None,
        "a fresh window carries no answer"
    );

    // -- Exec: the gate answers in this window.
    state.transcript.set_diff_gate(true);

    // -- Check: the answer is reused until the next frame reads the index.
    assert_eq!(state.transcript.diff_gate(), Some(true));
    assert_eq!(state.transcript.diff_gate(), Some(true));
    state.transcript.ensure_height_index(width);
    assert_eq!(
        state.transcript.diff_gate(),
        None,
        "the next paint window must ask again"
    );

    // -- Setup & Fixtures: a block re-keyed onto a new call.
    state.transcript.set_diff_gate(false);
    state
        .transcript
        .key_assistant_stream(crate::test_support::inference());

    // -- Check: a new call's block carries no answer of the old one.
    assert_eq!(
        state.transcript.diff_gate(),
        None,
        "a re-keyed block must not inherit the old answer"
    );
    Ok(())
}

// -- Test Support

/// One prose line of the folded fixture text, long enough to wrap at the test
/// width.
fn prose_line(index: usize) -> String {
    format!("line {index} of the streamed answer carries enough words to wrap once")
}

/// An assistant entry carrying `markdown`, as a fold leaves it.
fn assistant_entry_with(markdown: &str) -> TranscriptEntry {
    TranscriptEntry::Message(MessageEntry {
        role: MessageRole::Assistant,
        markdown: markdown.to_string(),
        timestamp_millis: None,
        model: None,
        agent: None,
    })
}

/// Assert the store's cached geometry equals a wholesale rebuild of its own
/// entries at `width`.
fn assert_geometry_matches_wholesale(state: &AppState, width: usize) -> Result<()> {
    let mut wholesale = state.transcript.clone();
    wholesale.rebuild_height_index(width);
    assert_eq!(
        state.transcript.total_visual_rows(),
        wholesale.total_visual_rows(),
        "the deferred measure must match a wholesale rebuild"
    );
    let len = state.transcript.len();
    assert_eq!(
        state.transcript.frame_plan(0, len, width),
        wholesale.frame_plan(0, len, width),
        "the frame plan must match a wholesale rebuild"
    );
    Ok(())
}
