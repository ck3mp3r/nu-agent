use super::*;

/// The transcript as flattened row text.
fn lines(state: &AppState) -> Vec<String> {
    state.transcript.entries().iter().map(entry_text).collect()
}

/// A split turn's first pass, as the runtime streams it: the history summary
/// into the block.
fn stream_history_pass(state: &mut AppState) {
    reduce_compaction(state, started(CompactionSource::Auto));
    reduce_compaction(
        state,
        chunk_at(
            CompactionSource::Auto,
            SummaryStage::History,
            "history ",
            "history ",
        ),
    );
    reduce_compaction(
        state,
        chunk_at(
            CompactionSource::Auto,
            SummaryStage::History,
            "summary",
            "history summary",
        ),
    );
}

#[test]
fn compaction_history_chunks_stream_the_block() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    reduce_compaction(&mut state, started(CompactionSource::Auto));

    // -- Exec
    let changed = reduce_compaction(
        &mut state,
        chunk_at(
            CompactionSource::Auto,
            SummaryStage::History,
            "history summary",
            "history summary",
        ),
    );

    // -- Check: the history pass is the render lane; the block streams as it
    // always has.
    assert!(changed, "a history chunk repaints the block");
    assert!(
        lines(&state).iter().any(|line| line == "history summary"),
        "the history pass's text is the block's row, got: {}",
        lines(&state).join(" | ")
    );
    Ok(())
}

#[test]
fn compaction_turn_prefix_chunks_change_nothing() -> Result<()> {
    // -- Setup & Fixtures: the history pass has streamed its block.
    let mut state = AppState::default();
    stream_history_pass(&mut state);
    let before = lines(&state);
    let stream_start = state.transcript.summary_stream_start;

    // -- Exec: the split turn's earlier part streams its own aggregated text.
    let first = reduce_compaction(
        &mut state,
        chunk_at(
            CompactionSource::Auto,
            SummaryStage::TurnPrefix,
            "prefix ",
            "prefix ",
        ),
    );
    let second = reduce_compaction(
        &mut state,
        chunk_at(
            CompactionSource::Auto,
            SummaryStage::TurnPrefix,
            "summary",
            "prefix summary",
        ),
    );

    // -- Check: no repaint and no row — the block the history pass painted
    // keeps its rows, and the prefix pass paints nothing mid-flight.
    assert!(!first, "a turn-prefix chunk repaints nothing");
    assert!(!second, "a turn-prefix chunk repaints nothing");
    assert_eq!(before, lines(&state), "the block's rows are untouched");
    assert_eq!(
        state.transcript.summary_stream_start, stream_start,
        "the streaming block keeps the start the history pass set"
    );
    assert!(
        !lines(&state).iter().any(|line| line.contains("prefix")),
        "the prefix pass appends no row, got: {}",
        lines(&state).join(" | ")
    );
    Ok(())
}

#[test]
fn compaction_completion_replaces_the_history_stream_alone() -> Result<()> {
    // -- Setup & Fixtures: the history pass streamed, the prefix pass rendered
    // nothing, and the completion carries the merged summary.
    let mut state = AppState::default();
    stream_history_pass(&mut state);
    reduce_compaction(
        &mut state,
        chunk_at(
            CompactionSource::Auto,
            SummaryStage::TurnPrefix,
            "prefix summary",
            "prefix summary",
        ),
    );

    // -- Exec
    reduce_compaction(
        &mut state,
        completed(CompactionSource::Auto, "history summary\n\nprefix summary"),
    );

    // -- Check: the final block is the merged summary, alone. The streamed
    // history row is replaced, and the prefix pass left nothing to replace.
    assert_eq!(
        assistant_markdown_entries(&state),
        vec!["history summary\n\nprefix summary".to_string()],
        "the completion replaces the streamed block with the merged summary"
    );
    assert!(state.transcript.summary_stream_start.is_none());
    Ok(())
}
