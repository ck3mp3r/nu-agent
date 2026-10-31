//! The compaction events: payload shapes, subscription, and the
//! `summary_chunk` exclusion.
//!
//! Every fixture publishes on `bus.compaction()`, the channel the production
//! publishers use. No production code sends a compaction event on `ui_event`,
//! so a fixture on that channel would prove nothing.

use super::*;

#[tokio::test]
async fn test_hooks_runner_compaction_started_invokes_with_source() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::CompactionStarted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .compaction()
        .send(SystemEvent::CompactionStarted {
            source: "auto".to_string(),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "a compaction-started-only hook must subscribe bus.compaction(); got: {calls:?}"
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
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::CompactionCompleted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .compaction()
        .send(SystemEvent::CompactionCompleted {
            source: "manual".to_string(),
            summary_preview: "The session covered the parser rewrite".to_string(),
            summary_body: "The session covered the parser rewrite in full detail".to_string(),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "a compaction-completed-only hook must subscribe bus.compaction(); got: {calls:?}"
    );
    assert_eq!(calls[0].event, "compaction_completed");
    let body = payload(&calls[0])?;
    assert_eq!(body["source"], "manual");
    assert_eq!(
        body["summary_preview"],
        "The session covered the parser rewrite"
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
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::CompactionFailed])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .compaction()
        .send(SystemEvent::CompactionFailed {
            source: "auto".to_string(),
            message: "the summarizer returned nothing".to_string(),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "a compaction-failed-only hook must subscribe bus.compaction(); got: {calls:?}"
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
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::CompactionCompleted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .session()
        .send(SessionEvent::Started {
            session_id: "sess-1".to_string(),
            hydrated: false,
        })
        .await;
    let _ = bus
        .compaction()
        .send(SystemEvent::CompactionCompleted {
            source: "auto".to_string(),
            summary_preview: "preview".to_string(),
            summary_body: "body".to_string(),
        })
        .await;
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
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook(
        "audit",
        &[
            HookEvent::CompactionStarted,
            HookEvent::CompactionCompleted,
            HookEvent::CompactionFailed,
        ],
    )];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .compaction()
        .send(SystemEvent::CompactionSummaryChunk {
            source: "auto".to_string(),
            delta: "chunk".to_string(),
            aggregated: "chunk".to_string(),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    assert!(
        process.calls().is_empty(),
        "a streamed summary chunk must not spawn a hook: one subprocess per chunk is unbounded volume"
    );
    Ok(())
}
