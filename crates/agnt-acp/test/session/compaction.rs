//! Compaction events reach the ACP client through the turn drain.
//!
//! The translator's compaction arms are unit-tested in isolation, so those
//! tests pass whether or not anything feeds them. These tests publish on
//! `bus.compaction()` — the channel the production publishers use — while a
//! turn drains, which is the only way to prove the arms are live.

use super::*;

/// The `nu-agent` compaction object of a `session_info_update`, if the update
/// carries one.
fn compaction_meta(update: &SessionUpdate) -> Option<serde_json::Value> {
    let SessionUpdate::SessionInfoUpdate(info) = update else {
        return None;
    };
    info.meta
        .as_ref()
        .and_then(|meta| meta.get("nu-agent"))
        .cloned()
}

/// Every compaction state the sink recorded, in order.
fn recorded_compaction_states(sink: &RecordingSink) -> Vec<String> {
    sink.taken()
        .iter()
        .filter_map(compaction_meta)
        .filter_map(|meta| {
            meta.get("state")
                .and_then(|state| state.as_str())
                .map(|state| state.to_string())
        })
        .collect()
}

#[tokio::test]
async fn test_session_manager_prompt_delivers_compaction_events_to_the_client() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let sink = RecordingSink::default();
    let bus = manager
        .get_session(&session_id)
        .ok_or("session must exist")?
        .bus()
        .clone();

    // Park the turn on the session's runtime lock. `prompt()` subscribes its
    // bus receivers before it takes the lock, so the compaction events
    // published while it is parked are buffered for the drain.
    let runtime = Arc::clone(
        manager
            .get_session(&session_id)
            .ok_or("session must exist")?
            .runtime(),
    );
    let guard = runtime.lock().await;
    let prompt = manager.prompt(&session_id, "hello".to_string(), &sink);
    tokio::pin!(prompt);
    let parked = tokio::time::timeout(Duration::from_millis(50), &mut prompt).await;
    assert!(
        parked.is_err(),
        "the prompt must park on the runtime lock before the turn starts"
    );

    // -- Exec
    let _ = bus
        .compaction()
        .send(SystemEvent::CompactionStarted {
            source: "auto".to_string(),
        })
        .await;
    let _ = bus
        .compaction()
        .send(SystemEvent::CompactionCompleted {
            source: "auto".to_string(),
            summary_preview: "the session covered the parser rewrite".to_string(),
            summary_body: "the session covered the parser rewrite in full detail".to_string(),
        })
        .await;
    let _ = bus
        .compaction()
        .send(SystemEvent::CompactionFailed {
            source: "auto".to_string(),
            message: "the summarizer returned nothing".to_string(),
        })
        .await;
    drop(guard);
    let _ = prompt.await?;

    // -- Check
    assert_eq!(
        recorded_compaction_states(&sink),
        vec![
            "started".to_string(),
            "completed".to_string(),
            "failed".to_string()
        ],
        "every compaction event published during the turn must reach the client"
    );
    let details: Vec<serde_json::Value> = sink.taken().iter().filter_map(compaction_meta).collect();
    assert_eq!(
        details[1].get("source").and_then(|value| value.as_str()),
        Some("auto"),
        "the completed update must carry the source: {details:?}"
    );
    assert_eq!(
        details[2].get("detail").and_then(|value| value.as_str()),
        Some("the summarizer returned nothing"),
        "the failed update must carry the message as its detail: {details:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_prompt_without_compaction_emits_no_compaction_update() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let sink = RecordingSink::default();

    // -- Exec
    let _ = manager
        .prompt(&session_id, "hello".to_string(), &sink)
        .await?;

    // -- Check
    assert!(
        recorded_compaction_states(&sink).is_empty(),
        "a turn that compacts nothing must emit no compaction update; got: {:?}",
        sink.taken()
    );
    Ok(())
}
