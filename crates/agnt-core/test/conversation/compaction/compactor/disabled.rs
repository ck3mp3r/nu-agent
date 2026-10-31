use super::*;

/// A compactor built without a planning spec is disabled: `compact` must emit
/// one `Failed` event and return `Err`, never a silent success.
#[tokio::test]
async fn disabled_compactor_emits_failed_and_returns_error() -> Result<()> {
    // -- Setup & Fixtures: the 4th argument is the planning spec; `None` means
    // the provider resolved to no rig provider id.
    let store = memory_store()?;
    seed(&*store)?;
    let bus = Bus::default();
    let mut rx = bus.compaction().subscribe();
    let compactor = NuCompactor::new(stream_model(&["summary"]).erase(), bus, test_policy(), None);

    // -- Exec
    let result = compactor
        .compact(&*store, CONV, &CompactReason::Threshold, "test_source")
        .await;

    // -- Check
    let Err(MemoryError::Backend(err)) = result else {
        panic!("a disabled compactor must fail compaction, got {result:?}");
    };
    assert!(
        err.to_string().starts_with("Compaction is disabled:"),
        "the error must name the disabled compaction, got: {err}"
    );
    let mut failed = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let SystemEvent::CompactionFailed { message, .. } = event {
            failed.push(message);
        }
    }
    assert_eq!(
        failed.len(),
        1,
        "a disabled compactor must emit exactly one Failed event, got {failed:?}"
    );
    assert!(
        failed[0].starts_with("Compaction is disabled:"),
        "the Failed event must name the disabled compaction, got {failed:?}"
    );
    Ok(())
}
