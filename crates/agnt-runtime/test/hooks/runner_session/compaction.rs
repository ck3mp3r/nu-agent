//! The compaction events: payload shapes, subscription, and the
//! `summary_chunk` exclusion.
//!
//! Every fixture publishes on `bus.lifecycle()` or `bus.stream()`, the channels
//! the production publishers use. No production code sends a compaction event
//! on any other channel, so a fixture on one would prove nothing.

use super::*;

#[tokio::test]
async fn test_hooks_runner_compaction_started_invokes_with_source() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::CompactionStarted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus.lifecycle().publish(LifecycleEvent::CompactionStarted {
        source: CompactionSource::Auto,
    });
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "a compaction-started-only hook must subscribe the lifecycle channel; got: {calls:?}"
    );
    assert_eq!(calls[0].event, "compaction_started");
    let body = payload(&calls[0])?;
    assert_eq!(body["source"], "auto");
    assert_eq!(
        body.as_object().map(|object| object.len()),
        Some(1),
        "the payload carries the source only: {body}"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_compaction_completed_carries_preview_and_no_body() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::CompactionCompleted])];
    let handles = spawn(&bus, &hooks, &process);
    let summary = "The session covered the parser rewrite. ".repeat(20);

    // -- Exec
    let _ = bus
        .lifecycle()
        .publish(LifecycleEvent::CompactionCompleted {
            source: CompactionSource::Manual,
            summary: summary.clone(),
        });
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "a compaction-completed-only hook must subscribe the lifecycle channel; got: {calls:?}"
    );
    assert_eq!(calls[0].event, "compaction_completed");
    let body = payload(&calls[0])?;
    assert_eq!(body["source"], "slash");
    assert_eq!(
        body["summary_preview"],
        summary.chars().take(200).collect::<String>()
    );
    assert!(
        body.get("summary_body").is_none(),
        "the payload must not carry the unbounded summary body: {body}"
    );
    assert_eq!(
        body.as_object().map(|object| object.len()),
        Some(2),
        "the payload carries the source and the preview: {body}"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_compaction_failed_carries_message() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::CompactionFailed])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus.lifecycle().publish(LifecycleEvent::CompactionFailed {
        source: CompactionSource::Auto,
        message: "the summarizer returned nothing".to_string(),
    });
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "a compaction-failed-only hook must subscribe the lifecycle channel; got: {calls:?}"
    );
    assert_eq!(calls[0].event, "compaction_failed");
    let body = payload(&calls[0])?;
    assert_eq!(body["source"], "auto");
    assert_eq!(body["message"], "the summarizer returned nothing");
    assert_eq!(
        body.as_object().map(|object| object.len()),
        Some(2),
        "the payload carries the source and the message: {body}"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_compaction_events_stamp_session_id() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::CompactionCompleted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus.lifecycle().publish(LifecycleEvent::SessionStarted {
        session_id: "sess-1".to_string(),
        hydrated: false,
    });
    let _ = bus
        .lifecycle()
        .publish(LifecycleEvent::CompactionCompleted {
            source: CompactionSource::Auto,
            summary: "preview".to_string(),
        });
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "only the compaction event invokes; got: {calls:?}"
    );
    assert_eq!(
        envelope_json(&calls[0])?["session_id"],
        "sess-1",
        "Started must stamp the tracked session id"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_compaction_summary_chunk_invokes_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let process = MockProcess::default();
    // Declaring a tool event subscribes the stream, so the chunk below is
    // actually delivered to the runner rather than dropped as unpublishable.
    let hooks = vec![hook(
        "audit",
        &[
            HookEvent::CompactionStarted,
            HookEvent::CompactionCompleted,
            HookEvent::CompactionFailed,
            HookEvent::ToolStarted,
        ],
    )];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .stream()
        .publish(SessionRecord::Compaction(CompactionRecord::SummaryChunk {
            source: CompactionSource::Auto,
            delta: "chunk".to_string(),
            aggregated: "chunk".to_string(),
        }));
    drop(bus);
    finish(handles).await?;

    // -- Check
    assert!(
        process.calls().is_empty(),
        "a streamed summary chunk must not spawn a hook: one subprocess per chunk is unbounded volume"
    );
    Ok(())
}
