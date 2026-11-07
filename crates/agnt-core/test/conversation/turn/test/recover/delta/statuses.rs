use super::*;

/// WHEN a turn is persisted, THE settle unit SHALL carry the structural status
/// of every call the turn dispatched: the variant a settled call built at its
/// set point, and the cancelled variant for a call the turn stopped before it
/// completed.
#[tokio::test]
async fn persist_turn_delta_writes_the_structural_status_of_every_call() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = MemoryState::new(
        RigStoreConfig::new(agnt_runtime::session::StoreType::Sqlite)
            .with_cache_dir(temp_dir.path()),
    );
    let session_id = "test-recover-statuses";
    let full_history = vec![Message::user("q2")];
    let mut cancelled = record("tc2", Some("command timed out after 120 seconds"))?;
    cancelled.success = Some(false);
    cancelled.status = Some(ToolStatus::Cancelled);
    cancelled.error_kind = Some("cancelled".to_string());
    let records = vec![
        record("tc1", Some("file contents"))?,
        cancelled,
        record("tc3", None)?,
    ];

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "q2",
            &full_history,
            0,
            INTERRUPTED_REPLY,
            recovery(&records, None),
        )
        .await;

    // -- Check: the statuses are the ones the records settled with, read back
    // from the settle unit the recovery wrote.
    let facts = memory_state.inner_memory().facts().await?;
    let statuses = facts.read(session_id).await?.status_map();
    assert_eq!(
        statuses.get("tc1").copied(),
        Some(ToolStatus::Success),
        "a settled success keeps its variant; got {statuses:?}"
    );
    assert_eq!(
        statuses.get("tc2").copied(),
        Some(ToolStatus::Cancelled),
        "a cancelled run keeps its variant, whatever its result text; got {statuses:?}"
    );
    assert_eq!(
        statuses.get("tc3").copied(),
        Some(ToolStatus::Cancelled),
        "a call the turn never settled is persisted as cancelled; got {statuses:?}"
    );
    Ok(())
}
