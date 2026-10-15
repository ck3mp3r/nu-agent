use super::*;

#[tokio::test]
async fn marker_present_still_calls_llm() -> Result<()> {
    // -- Setup & Fixtures
    // A marker exists that already absorbed all the evicted messages. Compaction
    // must ALWAYS run the LLM — the marker summary is only used as carry-over,
    // never to skip the call.
    let (_, store, _marker_summary, _temp_dir) =
        seeded_marker_store("conv-1", "cached summary text").await?;
    let bus = Bus::default();
    let model = stream_model(&["fresh LLM summary"]);
    let compactor = make_compactor_with_store(model.clone(), bus, None, store);

    // -- Exec
    let evicted = vec![
        Message::user("m1"),
        Message::user("m2"),
        Message::user("m3"),
    ];
    let artifact = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e}"))?;

    // -- Check
    let msg: Message = artifact.into();
    assert_eq!(
        user_text(&msg),
        format!("{SUMMARY_HEADER}fresh LLM summary"),
        "compact must return the fresh LLM summary even when a marker exists"
    );
    assert_eq!(
        model.requests().len(),
        1,
        "compact must call the LLM even when a marker exists"
    );
    Ok(())
}

#[tokio::test]
async fn no_marker_calls_llm() -> Result<()> {
    // -- Setup & Fixtures
    // No store attached -> no marker -> LLM must be called as before.
    let bus = Bus::default();
    let model = stream_model(&["fresh summary"]);
    let compactor = make_compactor(model.clone(), bus, None);

    // -- Exec
    let evicted = vec![Message::user("old message")];
    let artifact = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e}"))?;

    // -- Check
    assert_eq!(
        model.requests().len(),
        1,
        "no marker must trigger the LLM call"
    );
    let msg: Message = artifact.into();
    assert_eq!(user_text(&msg), format!("{SUMMARY_HEADER}fresh summary"));
    Ok(())
}

#[tokio::test]
async fn new_messages_beyond_marker_calls_llm() -> Result<()> {
    // -- Setup & Fixtures
    // Marker absorbed 2, but 3 are evicted now -> LLM must run.
    let (_, store, _summary, _temp_dir) =
        seeded_marker_store("conv-1", "old cached summary").await?;
    let bus = Bus::default();
    let model = stream_model(&["new summary"]);
    let compactor = make_compactor_with_store(model.clone(), bus, None, store);

    // -- Exec
    let evicted = vec![
        Message::user("m1"),
        Message::user("m2"),
        Message::user("m3"),
    ];
    let artifact = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e}"))?;

    // -- Check
    assert_eq!(
        model.requests().len(),
        1,
        "a marker must not skip the LLM call"
    );
    let msg: Message = artifact.into();
    assert_eq!(user_text(&msg), format!("{SUMMARY_HEADER}new summary"));
    Ok(())
}

#[tokio::test]
async fn marker_summary_used_as_carry_over_when_in_process_none() -> Result<()> {
    // -- Setup & Fixtures
    // A marker exists, and the in-process carry_over is None (restart). The
    // marker summary must be prepended to the summarizer input.
    let (_, store, marker_summary, _temp_dir) =
        seeded_marker_store("conv-1", "carried forward summary").await?;
    let bus = Bus::default();
    let model = stream_model(&["new summary"]);
    let compactor = make_compactor_with_store(model.clone(), bus, None, store);

    // -- Exec
    let evicted = vec![
        Message::user("m1"),
        Message::user("m2"),
        Message::user("m3"),
    ];
    compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e}"))?;

    // -- Check
    let prompt = last_request_prompt(&model)?;
    assert!(
        prompt.contains(&marker_summary),
        "marker summary must be prepended as carry_over on restart, got: {prompt}"
    );
    Ok(())
}

#[tokio::test]
async fn marker_present_still_emits_full_lifecycle_events() -> Result<()> {
    // -- Setup & Fixtures
    // A marker exists that already absorbed all the evicted messages. Compaction
    // must still emit the full Started/SummaryChunk/Completed lifecycle.
    let (_, store, _summary, _temp_dir) =
        seeded_marker_store("conv-1", "cached summary text").await?;
    let bus = Bus::default();
    let mut rx = bus.compaction().subscribe();
    let model = stream_model(&["fresh LLM summary"]);
    let compactor = make_compactor_with_store(model, bus, None, store);

    // -- Exec
    let evicted = vec![
        Message::user("m1"),
        Message::user("m2"),
        Message::user("m3"),
    ];
    compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e}"))?;

    // -- Check
    let started = rx.recv().await.map_err(|_| "should receive Started")?;
    assert!(
        matches!(started, CompactionEvent::Started { .. }),
        "first event should be Started, got {started:?}"
    );
    let chunk = rx.recv().await.map_err(|_| "should receive SummaryChunk")?;
    assert!(
        matches!(chunk, CompactionEvent::SummaryChunk { .. }),
        "expected a SummaryChunk after Started, got {chunk:?}"
    );
    let completed = rx.recv().await.map_err(|_| "should receive Completed")?;
    match &completed {
        CompactionEvent::Completed { summary_body, .. } => {
            assert_eq!(
                summary_body, "fresh LLM summary",
                "Completed must carry the fresh LLM summary"
            );
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    Ok(())
}

#[tokio::test]
async fn marker_present_still_writes_a_new_marker() -> Result<()> {
    // -- Setup & Fixtures
    let (store, store_arc, _summary, _temp_dir) =
        seeded_marker_store("conv-1", "cached summary text").await?;
    let bus = Bus::default();
    let model = stream_model(&["fresh LLM summary"]);
    let compactor = make_compactor_with_store(model, bus, None, store_arc);

    // -- Exec
    let evicted = vec![
        Message::user("m1"),
        Message::user("m2"),
        Message::user("m3"),
    ];
    compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e}"))?;

    // -- Check
    let (_, entries) = store.load("conv-1").await?.ok_or("should find session")?;
    let markers: Vec<_> = entries
        .iter()
        .filter(|e| matches!(e, StoreEntry::Marker(_)))
        .collect();
    assert_eq!(
        markers.len(),
        2,
        "compact must append a new marker even when one already exists, found {}",
        markers.len()
    );
    Ok(())
}
