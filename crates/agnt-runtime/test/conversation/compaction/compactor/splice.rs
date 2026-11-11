use super::*;

/// Compact `store`, assert the splice landed under the `rig.compaction` author,
/// then undo it and assert the original conversation is back.
async fn assert_splice_and_undo(store: Arc<dyn Store>) -> Result<()> {
    // -- Setup & Fixtures
    seed(&*store)?;
    let bus = Bus::default();
    let compactor = make_compactor(&["rolled-up summary"], bus)?;

    // -- Exec
    let outcome = compactor
        .compact(
            Arc::clone(&store),
            CONV,
            &CompactReason::Threshold,
            CompactionSource::Auto,
        )
        .await?
        .ok_or("compaction should have run")?;

    // -- Check: the splice is the conversation rig now holds.
    let summary = outcome
        .messages
        .first()
        .ok_or("the spliced conversation is non-empty")?;
    let summary_text = user_text(summary);
    assert!(
        summary_text.starts_with(SUMMARY_PREFIX),
        "the first message must be the summary, got: {summary_text}"
    );
    assert!(
        summary_text.contains("rolled-up summary"),
        "the summary must carry the streamed text, got: {summary_text}"
    );

    let mut conversation = rig_memory::load(&*store, CONV)?;
    let first = conversation
        .entries()
        .next()
        .ok_or("the stored conversation is non-empty")?;
    assert_eq!(
        first.author,
        Some(&summary_author()),
        "the summary entry must be authored by rig.compaction"
    );
    assert!(
        conversation
            .asides()
            .any(|(_, aside)| aside.kind == rig_memory::COMPACTED),
        "the splice must record a rig.compaction aside"
    );
    assert!(
        rig_memory::summary_state(&conversation).tracked.is_empty(),
        "no track rules are configured, so the tracked set stays empty"
    );

    // -- Exec: undo the compaction.
    let undone = rig_memory::undo_summary(&mut conversation, &*store, CONV, summary_author())?;

    // -- Check: the replaced messages are back.
    assert_eq!(
        undone.restored.len(),
        3,
        "undo must restore the three replaced messages"
    );
    assert_eq!(
        conversation.messages().len(),
        4,
        "undo must leave the original four messages"
    );
    let restored_first = conversation
        .messages()
        .first()
        .ok_or("the restored conversation is non-empty")?;
    assert_eq!(user_text(restored_first), "first question");
    Ok(())
}

#[tokio::test]
async fn compact_splices_under_the_summary_author_on_the_sqlite_backend() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store().await?;

    // -- Exec & Check
    assert_splice_and_undo(Arc::clone(&store)).await
}

#[tokio::test]
async fn compact_splices_under_the_summary_author_on_the_file_backend() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().map_err(|_| "should create temp dir")?;
    let store = file_store(temp_dir.path()).await?;

    // -- Exec & Check
    assert_splice_and_undo(Arc::clone(&store)).await
}

#[tokio::test]
async fn compact_validates_the_summary_through_the_folded_partial_response() -> Result<()> {
    // -- Setup & Fixtures
    // The summary is read from `Streamed::partial()`, not from the deltas, so a
    // multi-chunk stream folds into one trimmed summary.
    let store = memory_store().await?;
    seed(&*store)?;
    let bus = Bus::default();
    let compactor = make_compactor(&["  part one ", "part two  "], bus)?;

    // -- Exec
    let outcome = compactor
        .compact(
            Arc::clone(&store),
            CONV,
            &CompactReason::Threshold,
            CompactionSource::Auto,
        )
        .await?
        .ok_or("compaction should have run")?;

    // -- Check
    let summary = outcome
        .messages
        .first()
        .ok_or("the spliced conversation is non-empty")?;
    let text = user_text(summary);
    assert!(
        text.contains("<summary>\npart one part two\n</summary>"),
        "the folded summary must be trimmed and whole, got: {text}"
    );
    Ok(())
}

#[tokio::test]
async fn compact_emits_started_chunks_and_completed() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store().await?;
    seed(&*store)?;
    let bus = Bus::default();
    let mut rx = bus.stream().subscribe();
    let compactor = make_compactor(&["first ", "second"], bus)?;

    // -- Exec
    compactor
        .compact(
            Arc::clone(&store),
            CONV,
            &CompactReason::Threshold,
            CompactionSource::Auto,
        )
        .await?;

    // -- Check
    let mut records = Vec::new();
    while let Ok(record) = rx.try_recv() {
        records.push(record);
    }
    assert!(
        matches!(
            records.first(),
            Some(SessionRecord::Compaction(CompactionRecord::Started {
                source: CompactionSource::Auto,
            }))
        ),
        "compaction must announce Started first, got {records:?}"
    );
    let chunks = records
        .iter()
        .filter(|record| {
            matches!(
                record,
                SessionRecord::Compaction(CompactionRecord::SummaryChunk { .. })
            )
        })
        .count();
    assert_eq!(
        chunks, 2,
        "one SummaryChunk per text delta, got {records:?}"
    );
    assert!(
        matches!(
            records.last(),
            Some(SessionRecord::Compaction(CompactionRecord::Completed { summary, .. }))
                if summary == "first second"
        ),
        "compaction must end with Completed carrying the summary, got {records:?}"
    );
    Ok(())
}

#[tokio::test]
async fn compact_returns_none_when_nothing_would_be_summarized() -> Result<()> {
    // -- Setup & Fixtures
    // A single-message conversation has nothing older to summarize.
    let store = memory_store().await?;
    let mut conversation = rig_memory::load(&*store, CONV)?;
    conversation.extend([Message::user("only message")]);
    rig_memory::save(&*store, CONV, &mut conversation)?;
    let bus = Bus::default();
    let compactor = make_compactor(&["unused summary"], bus)?;

    // -- Exec
    let outcome = compactor
        .compact(
            Arc::clone(&store),
            CONV,
            &CompactReason::Threshold,
            CompactionSource::Auto,
        )
        .await?;

    // -- Check
    assert!(
        outcome.is_none(),
        "a conversation with nothing older to summarize must not compact"
    );
    Ok(())
}

#[tokio::test]
async fn compact_is_disabled_without_a_model_spec() -> Result<()> {
    // -- Setup & Fixtures
    // An unresolved provider name disables compaction for the session.
    let store = memory_store().await?;
    seed(&*store)?;
    let bus = Bus::default();
    let mut rx = bus.stream().subscribe();
    let compactor = NuCompactor::new(
        stream_model(&["unused summary"]).erase(),
        bus,
        test_policy(),
        resolve_model_spec("no-such-vendor", "m", Some(8_000), None),
    );

    // -- Exec
    let result = compactor
        .compact(
            Arc::clone(&store),
            CONV,
            &CompactReason::Threshold,
            CompactionSource::Auto,
        )
        .await;

    // -- Check
    let Err(MemoryError::Backend(err)) = result else {
        panic!("a disabled compactor must fail compaction, got {result:?}");
    };
    assert!(
        err.to_string().starts_with("Compaction is disabled:"),
        "the error must name the disabled compaction, got: {err}"
    );
    let failed = recv_until_failed(&mut rx).await?;
    assert!(
        failed.starts_with("Compaction is disabled:"),
        "expected Failed naming the disabled compaction, got {failed:?}"
    );
    assert!(
        compactor.spec().is_none(),
        "the compactor must hold no spec when the provider name is unresolved"
    );
    Ok(())
}

#[tokio::test]
async fn resolve_model_spec_carries_the_configured_limits() -> Result<()> {
    // -- Setup & Fixtures
    // -- Exec
    let spec = test_spec()?;

    // -- Check
    assert_eq!(spec.context_window, Some(200_000));
    assert_eq!(spec.max_output_tokens, Some(1_024));
    assert_eq!(spec.id, "gpt-4o");
    Ok(())
}
