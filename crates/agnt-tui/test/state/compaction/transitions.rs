use super::*;

/// Compaction metadata is transcript UI chrome, not session payload: the
/// header stays "Compaction", the summary body is present, and no source
/// metadata or explanatory copy leaks into the transcript.
#[test]
fn compaction_metadata_is_ui_chrome_not_session_payload() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    reduce_compaction(&mut state, started("slash_compact"));
    reduce_compaction(
        &mut state,
        completed("slash_compact", "preview", "persisted summary body"),
    );

    // -- Check
    assert_eq!(
        state.transcript.blocks()[0].source.plain_text(),
        "Compaction",
        "metadata is transcript UI chrome, not session system summary payload"
    );
    assert!(
        state
            .transcript
            .blocks()
            .iter()
            .any(|line| line.source.plain_text() == "persisted summary body")
    );

    // -- Setup & Fixtures: an empty summary must not claim a persisted summary.
    let mut noop = AppState::default();
    reduce_compaction(&mut noop, started("auto_threshold"));
    reduce_compaction(
        &mut noop,
        completed("auto_threshold", "preview", "(empty summary)"),
    );

    // -- Check
    let lines = noop
        .transcript
        .blocks()
        .iter()
        .map(|line| line.source.plain_text())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"(empty summary)".to_string()));
    assert!(!lines.iter().any(|line| line.contains("source=")));
    assert!(!lines.iter().any(|line| line.contains(
        "metadata above is UI diagnostic only and NOT included in future LLM prompt history"
    )));
    assert!(!lines.iter().any(|line| line.contains(
        "Summary text below is persisted as system summary and IS included in future history"
    )));

    // -- Setup & Fixtures: both triggers render a block.
    let mut triggers = AppState::default();
    for source in ["slash_compact", "auto_threshold"] {
        reduce_compaction(&mut triggers, started(source));
        reduce_compaction(
            &mut triggers,
            completed(source, "preview", &format!("summary from {source}")),
        );
    }

    // -- Check
    let lines = triggers
        .transcript
        .blocks()
        .iter()
        .map(|line| line.source.plain_text())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"summary from slash_compact".to_string()));
    assert!(lines.contains(&"summary from auto_threshold".to_string()));
    Ok(())
}

/// Compaction completion and failure clear the status line; completion also
/// resets the latest total token count.
#[test]
fn compaction_completion_and_failure_bookkeeping() -> Result<()> {
    // -- Setup & Fixtures
    let mut completed_state = busy_state_with_clean_transcript();
    completed_state.status.message.set_message("Thinking...");
    completed_state.status.tokens.latest_total_tokens = Some(50_000);

    // -- Exec
    reduce_compaction(&mut completed_state, completed("test", "...", "summary"));

    // -- Check
    assert!(completed_state.status.message.status_line().is_empty());
    assert_eq!(completed_state.status.tokens.latest_total_tokens, None);

    // -- Setup & Fixtures
    let mut failed_state = busy_state_with_clean_transcript();
    failed_state.status.message.set_message("Thinking...");

    // -- Exec
    reduce_compaction(
        &mut failed_state,
        CompactionEvent::Failed {
            source: "test".to_string(),
            message: "err".to_string(),
        },
    );

    // -- Check
    assert!(failed_state.status.message.status_line().is_empty());
    Ok(())
}

/// Streaming compaction renders progressively, reprojects on each chunk
/// (replacing rather than appending), ignores empty chunks, and clears the
/// streaming state on completion.
#[test]
fn compaction_streaming_lifecycle() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    reduce_compaction(&mut state, started("auto"));

    // -- Exec & Check: chunks render progressively.
    reduce_compaction(&mut state, chunk("auto", "Hello", "Hello"));
    assert!(state.transcript.len() > 1, "should have header + content");
    reduce_compaction(&mut state, chunk("auto", " world", "Hello world"));
    reduce_compaction(&mut state, chunk("auto", " done", "Hello world done"));
    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .map(|line| line.source.plain_text())
        .collect();
    assert!(lines.iter().any(|l| l.contains("Hello world done")));

    // -- Exec & Check: a later chunk reprojects, replacing the old partial.
    let mut reproject = AppState::default();
    reduce_compaction(&mut reproject, started("auto"));
    reduce_compaction(&mut reproject, chunk("auto", "First", "First"));
    reduce_compaction(&mut reproject, chunk("auto", " Second", "First Second"));
    let lines: Vec<String> = reproject
        .transcript
        .blocks()
        .iter()
        .map(|line| line.source.plain_text())
        .collect();
    let first_only_count = lines
        .iter()
        .filter(|l| l.contains("First") && !l.contains("Second"))
        .count();
    assert_eq!(first_only_count, 0, "old partial render should be replaced");

    // -- Exec & Check: empty chunks are ignored.
    let mut empty = AppState::default();
    reduce_compaction(&mut empty, started("auto"));
    let after_start = empty.transcript.len();
    reduce_compaction(&mut empty, chunk("auto", "", ""));
    assert_eq!(empty.transcript.len(), after_start);
    assert!(empty.transcript.summary_stream_start.is_none());

    // -- Exec & Check: completion clears the streaming state.
    let mut finishing = AppState::default();
    reduce_compaction(&mut finishing, started("auto"));
    reduce_compaction(&mut finishing, chunk("auto", "text", "text"));
    assert!(finishing.transcript.summary_stream_start.is_some());
    reduce_compaction(&mut finishing, completed("auto", "text", "text"));
    assert!(finishing.transcript.summary_stream_start.is_none());

    // -- Exec & Check: finalize the progressive stream.
    reduce_compaction(
        &mut state,
        completed("auto", "Hello world done", "Hello world done"),
    );
    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .map(|line| line.source.plain_text())
        .collect();
    assert!(lines.iter().any(|l| l.contains("Hello world done")));
    Ok(())
}
