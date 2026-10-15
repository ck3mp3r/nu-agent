use super::*;

#[tokio::test]
async fn summarizer_error_maps_to_memory_error_backend() -> Result<()> {
    let bus = Bus::default();
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("partial".to_string()),
        MockStreamEvent::error("provider exploded"),
    ]]);
    let compactor = make_compactor(model, bus, None);

    let evicted = vec![Message::user("old message")];
    let result = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await;

    assert!(
        matches!(result, Err(MemoryError::Backend(_))),
        "expected Err(MemoryError::Backend), got {result:?}"
    );
    Ok(())
}

#[tokio::test]
async fn empty_summary_is_rejected_and_no_marker_written() -> Result<()> {
    // -- Setup & Fixtures
    // A stream that emits only reasoning (no text) leaves `aggregated` empty.
    // Compaction must reject it: emit Failed with "empty summary", return Err,
    // and NOT persist a marker.
    let (store, store_arc, _summary, _temp_dir) =
        seeded_marker_store("conv-1", "cached summary text").await?;
    let bus = Bus::default();
    let mut rx = bus.compaction().subscribe();
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::reasoning("thinking hard"),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let compactor = make_compactor_with_store(model, bus, None, store_arc);

    // -- Exec
    let evicted = vec![
        Message::user("m1"),
        Message::user("m2"),
        Message::user("m3"),
    ];
    let result = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await;

    // -- Check
    let Err(MemoryError::Backend(err)) = result else {
        panic!("compact must reject an empty summary, got {result:?}");
    };
    let message = err.to_string();
    assert!(
        message.contains("empty summary"),
        "error must mention the empty summary, got: {message}"
    );
    let failed = recv_until_failed(&mut rx).await?;
    assert!(
        failed.contains("empty summary"),
        "expected Failed with empty summary, got {failed:?}"
    );
    // No Completed event and no new marker persisted.
    let (_, entries) = store.load("conv-1").await?.ok_or("should find session")?;
    let markers: Vec<_> = entries
        .iter()
        .filter(|e| matches!(e, StoreEntry::Marker(_)))
        .collect();
    assert_eq!(
        markers.len(),
        1,
        "empty summary must NOT write a new marker, found {}",
        markers.len()
    );
    Ok(())
}

#[tokio::test]
async fn stream_error_surfaces_actual_error_message() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let mut rx = bus.compaction().subscribe();
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("partial".to_string()),
        MockStreamEvent::error("provider exploded"),
    ]]);
    let compactor = make_compactor(model, bus, None);

    // -- Exec
    let evicted = vec![Message::user("old message")];
    let result = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await;

    // -- Check
    let Err(MemoryError::Backend(err)) = result else {
        panic!("compact must surface the stream error, got {result:?}");
    };
    let message = err.to_string();
    assert!(
        message.contains("provider exploded"),
        "error must carry the actual stream error message, got: {message}"
    );
    let failed = recv_until_failed(&mut rx).await?;
    assert!(
        failed.contains("provider exploded"),
        "Failed event must carry the actual stream error, got {failed:?}"
    );
    Ok(())
}

#[tokio::test]
async fn tool_call_chunk_during_compaction_emits_failed() -> Result<()> {
    // -- Setup & Fixtures
    // No tools are registered during compaction, so a ToolCall chunk is
    // unexpected and must emit Failed rather than being swallowed.
    let bus = Bus::default();
    let mut rx = bus.compaction().subscribe();
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("tc1", "some_tool", serde_json::json!({})),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let compactor = make_compactor(model, bus, None);

    // -- Exec
    let evicted = vec![Message::user("old message")];
    let result = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await;

    // -- Check
    let Err(MemoryError::Backend(err)) = result else {
        panic!("compact must fail on a tool call chunk, got {result:?}");
    };
    let message = err.to_string();
    assert!(
        message.contains("tool call"),
        "error must mention the unexpected tool call, got: {message}"
    );
    let failed = recv_until_failed(&mut rx).await?;
    assert!(
        failed.contains("tool call"),
        "expected Failed mentioning tool call, got {failed:?}"
    );
    Ok(())
}

#[tokio::test]
async fn unknown_stream_chunk_emits_failed() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let mut rx = bus.compaction().subscribe();
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::unknown(serde_json::json!({"type": "mystery"})),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let compactor = make_compactor(model, bus, None);

    // -- Exec
    let result = compactor
        .compact(
            "conv-1",
            &[Message::user("old message")],
            None,
            "test_source",
        )
        .await;

    // -- Check
    let Err(MemoryError::Backend(err)) = result else {
        panic!("an unknown chunk must fail compaction, got {result:?}");
    };
    assert!(
        err.to_string().contains("unknown"),
        "the error must name the unknown chunk, got: {err}"
    );
    let failed = recv_until_failed(&mut rx).await?;
    assert!(
        failed.contains("unknown"),
        "expected Failed mentioning the unknown chunk, got {failed:?}"
    );
    Ok(())
}

// region:    --- Test Support

/// A store whose `load` and `append` fail, for the compactor's error paths.
#[derive(Debug, Clone)]
struct FailingStore {
    fail_load: bool,
}

impl SessionStore for FailingStore {
    type Error = std::io::Error;

    async fn create(
        &self,
        _id: &str,
        _first_messages: &[Message],
    ) -> core::result::Result<(), Self::Error> {
        Ok(())
    }

    async fn load(
        &self,
        _id: &str,
    ) -> core::result::Result<Option<(crate::session::SessionMetadata, Vec<StoreEntry>)>, Self::Error>
    {
        if self.fail_load {
            Err(std::io::Error::other("load failed"))
        } else {
            Ok(None)
        }
    }

    async fn append(
        &self,
        _id: &str,
        _entries: &[StoreEntry],
    ) -> core::result::Result<(), Self::Error> {
        Err(std::io::Error::other("append failed"))
    }

    async fn replace_entries(
        &self,
        _id: &str,
        _entries: &[StoreEntry],
    ) -> core::result::Result<(), Self::Error> {
        Ok(())
    }

    async fn list(&self) -> core::result::Result<Vec<crate::session::SessionInfo>, Self::Error> {
        Ok(Vec::new())
    }

    async fn delete(&self, _id: &str) -> core::result::Result<(), Self::Error> {
        Ok(())
    }
}

// endregion: --- Test Support

#[tokio::test]
async fn store_load_failure_emits_failed_and_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let mut rx = bus.compaction().subscribe();
    let model = stream_model(&["summary"]);
    let store = Arc::new(FailingStore { fail_load: true });
    let compactor = make_compactor_with_store(model, bus, None, store);

    // -- Exec
    let result = compactor
        .compact(
            "conv-1",
            &[Message::user("old message")],
            None,
            "test_source",
        )
        .await;

    // -- Check
    let Err(MemoryError::Backend(err)) = result else {
        panic!("a store load failure must fail compaction, got {result:?}");
    };
    assert!(
        err.to_string().contains("load failed"),
        "the error must carry the store failure, got: {err}"
    );
    let failed = recv_until_failed(&mut rx).await?;
    assert!(
        failed.contains("load failed"),
        "expected Failed carrying the store failure, got {failed:?}"
    );
    Ok(())
}

#[tokio::test]
async fn marker_append_failure_emits_failed_and_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let mut rx = bus.compaction().subscribe();
    let model = stream_model(&["summary"]);
    let store = Arc::new(FailingStore { fail_load: false });
    let compactor = make_compactor_with_store(model, bus, None, store);

    // -- Exec
    let result = compactor
        .compact(
            "conv-1",
            &[Message::user("old message")],
            None,
            "test_source",
        )
        .await;

    // -- Check
    let Err(MemoryError::Backend(err)) = result else {
        panic!("a marker append failure must fail compaction, got {result:?}");
    };
    assert!(
        err.to_string().contains("append failed"),
        "the error must carry the store failure, got: {err}"
    );
    let failed = recv_until_failed(&mut rx).await?;
    assert!(
        failed.contains("append failed"),
        "expected Failed carrying the store failure, got {failed:?}"
    );
    Ok(())
}
