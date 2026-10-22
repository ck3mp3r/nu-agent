use super::*;

#[tokio::test]
async fn stream_error_surfaces_actual_error_message() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store()?;
    seed(&*store)?;
    let bus = Bus::default();
    let mut rx = bus.compaction().subscribe();
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("partial".to_string()),
        MockStreamEvent::error("provider exploded"),
    ]]);
    let compactor = NuCompactor::new(model.erase(), bus, test_policy(), Some(test_spec()?));

    // -- Exec
    let result = compactor
        .compact(&*store, CONV, &CompactReason::Threshold, "test_source")
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
async fn empty_summary_is_rejected_and_nothing_is_spliced() -> Result<()> {
    // -- Setup & Fixtures
    // A stream that emits only reasoning (no text) leaves the folded response
    // empty. Compaction must reject it: emit Failed, return Err, and leave the
    // stored conversation untouched.
    let store = memory_store()?;
    seed(&*store)?;
    let bus = Bus::default();
    let mut rx = bus.compaction().subscribe();
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::reasoning("thinking hard"),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let compactor = NuCompactor::new(model.erase(), bus, test_policy(), Some(test_spec()?));

    // -- Exec
    let result = compactor
        .compact(&*store, CONV, &CompactReason::Threshold, "test_source")
        .await;

    // -- Check
    let Err(MemoryError::Backend(err)) = result else {
        panic!("compact must reject an empty summary, got {result:?}");
    };
    assert!(
        err.to_string().contains("no summary"),
        "error must name the empty summary, got: {err}"
    );
    let failed = recv_until_failed(&mut rx).await?;
    assert!(
        failed.contains("no summary"),
        "expected Failed naming the empty summary, got {failed:?}"
    );
    let conversation = rig_memory::load(&*store, CONV)?;
    assert_eq!(
        conversation.messages().len(),
        4,
        "a rejected summary must not splice the conversation"
    );
    Ok(())
}

#[tokio::test]
async fn tool_call_chunk_during_compaction_emits_failed() -> Result<()> {
    // -- Setup & Fixtures
    // No tools are registered during compaction, so a ToolCall chunk is
    // unexpected and must emit Failed rather than being swallowed.
    let store = memory_store()?;
    seed(&*store)?;
    let bus = Bus::default();
    let mut rx = bus.compaction().subscribe();
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("tc1", "some_tool", serde_json::json!({})),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let compactor = NuCompactor::new(model.erase(), bus, test_policy(), Some(test_spec()?));

    // -- Exec
    let result = compactor
        .compact(&*store, CONV, &CompactReason::Threshold, "test_source")
        .await;

    // -- Check
    let Err(MemoryError::Backend(err)) = result else {
        panic!("compact must fail on a tool call chunk, got {result:?}");
    };
    assert!(
        err.to_string().contains("tool call"),
        "error must mention the unexpected tool call, got: {err}"
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
    let store = memory_store()?;
    seed(&*store)?;
    let bus = Bus::default();
    let mut rx = bus.compaction().subscribe();
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::unknown(serde_json::json!({"type": "mystery"})),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let compactor = NuCompactor::new(model.erase(), bus, test_policy(), Some(test_spec()?));

    // -- Exec
    let result = compactor
        .compact(&*store, CONV, &CompactReason::Threshold, "test_source")
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

#[tokio::test]
async fn store_load_failure_emits_failed_and_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let store = FailingStore {
        inner: memory_store()?,
        fail_read: true,
    };
    let bus = Bus::default();
    let mut rx = bus.compaction().subscribe();
    let compactor = make_compactor(&["summary"], bus)?;

    // -- Exec
    let result = compactor
        .compact(&store, CONV, &CompactReason::Threshold, "test_source")
        .await;

    // -- Check
    let Err(MemoryError::Backend(err)) = result else {
        panic!("a store load failure must fail compaction, got {result:?}");
    };
    assert!(
        err.to_string().contains("log read failed"),
        "the error must carry the store failure, got: {err}"
    );
    let failed = recv_until_failed(&mut rx).await?;
    assert!(
        failed.contains("log read failed"),
        "expected Failed carrying the store failure, got {failed:?}"
    );
    Ok(())
}

#[tokio::test]
async fn store_save_failure_emits_failed_and_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let inner = memory_store()?;
    seed(&*inner)?;
    let store = FailingStore {
        inner,
        fail_read: false,
    };
    let bus = Bus::default();
    let mut rx = bus.compaction().subscribe();
    let compactor = make_compactor(&["summary"], bus)?;

    // -- Exec
    let result = compactor
        .compact(&store, CONV, &CompactReason::Threshold, "test_source")
        .await;

    // -- Check
    let Err(MemoryError::Backend(err)) = result else {
        panic!("a store save failure must fail compaction, got {result:?}");
    };
    assert!(
        err.to_string().contains("log append failed"),
        "the error must carry the store failure, got: {err}"
    );
    let failed = recv_until_failed(&mut rx).await?;
    assert!(
        failed.contains("log append failed"),
        "expected Failed carrying the store failure, got {failed:?}"
    );
    Ok(())
}

// region:    --- Test Support

/// A rig store that reads through to `inner` and refuses every log write, for
/// the compactor's load and save failure paths.
struct FailingStore {
    inner: Arc<dyn Store>,
    fail_read: bool,
}

impl rig_memory::Files for FailingStore {
    fn write_file(&self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.inner.write_file(name, bytes)
    }

    fn append_file(&self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.inner.append_file(name, bytes)
    }

    fn read_file(&self, name: &str) -> std::io::Result<Vec<u8>> {
        self.inner.read_file(name)
    }

    fn remove_file(&self, name: &str) -> std::io::Result<()> {
        self.inner.remove_file(name)
    }

    fn files(&self) -> std::io::Result<Vec<String>> {
        self.inner.files()
    }
}

impl Store for FailingStore {
    fn logs(&self) -> std::io::Result<Vec<String>> {
        self.inner.logs()
    }

    fn read_log(&self, name: &str) -> std::io::Result<Vec<u8>> {
        if self.fail_read {
            Err(std::io::Error::other("log read failed"))
        } else {
            self.inner.read_log(name)
        }
    }

    fn append_log(
        &self,
        _name: &str,
        _len: u64,
        _bytes: &[u8],
    ) -> core::result::Result<(), rig_memory::LogWriteError> {
        Err(std::io::Error::other("log append failed").into())
    }

    fn replace_log(
        &self,
        _name: &str,
        _len: u64,
        _bytes: &[u8],
    ) -> core::result::Result<(), rig_memory::LogWriteError> {
        Err(std::io::Error::other("log replace failed").into())
    }

    fn destroy(&self) -> std::io::Result<()> {
        Ok(())
    }
}

// endregion: --- Test Support
