//! Store behavior: an unreachable store surfaces one refusal line and latches
//! itself, and a store that opens again clears the latch.

use super::super::*;

use crate::hook::chain::in_place::InPlace;

#[tokio::test]
async fn decide_compaction_surfaces_store_error_as_failed() -> Result<()> {
    // -- Setup & Fixtures
    // A rig store that cannot be opened: its cache directory's parent is a
    // regular file, so the database path is unusable.
    let temp_dir = TempDir::new().map_err(|_| "should create temp dir")?;
    let blocked = temp_dir.path().join("blocked");
    std::fs::write(&blocked, b"not a directory").map_err(|_| "should write the blocker file")?;
    let memory = Arc::new(CachedMemory::new(
        crate::session::RigStoreConfig::new(crate::session::StoreType::Sqlite)
            .with_cache_dir(blocked),
    ));
    let bus = Bus::default();
    let mut rx = bus.lifecycle().subscribe();
    let compaction = CompactionConfig {
        compactor: NuCompactor::new(
            MockCompletionModel::text("summary").erase(),
            Bus::default(),
            rig_memory::CompactionPolicy::default(),
            test_model_spec(),
        ),
        reserve_pct: crate::config::defaults::DEFAULT_RESERVE_PCT,
    };
    let meter = crate::hook::AnchorMeter::default();
    let pending = crate::hook::chain::PendingSummary::default();
    let spec = spec_with_input_limit(60_000)?;
    let model = compaction.compactor.model();

    // -- Exec
    decide_compaction(
        &Message::user("hi"),
        "conv-1",
        InPlace {
            spec: Some(&spec),
            pending: &pending,
        },
        &CompactionPath::new(
            memory.as_ref(),
            &compaction,
            &meter,
            &Default::default(),
            &model,
        ),
        bus.stream(),
        bus.lifecycle(),
    )
    .await;

    // -- Check
    let failed = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .map_err(|_| "should receive Failed within timeout")?
        .map_err(|_| "should receive Failed")?;
    assert!(
        matches!(&failed, crate::bus::LifecycleEvent::CompactionFailed { .. }),
        "an unusable rig store must be surfaced in a Failed event, got {failed:?}"
    );
    Ok(())
}

/// WHEN a store refuses to open, THE decision SHALL latch it and emit exactly
/// one clean line; the next decision SHALL make no further attempt.
#[tokio::test]
async fn store_dead_latch_makes_one_clean_line_and_no_further_attempt() -> Result<()> {
    // -- Setup & Fixtures: a foreign database every open refuses.
    let temp_dir = TempDir::new().map_err(|_| "should create temp dir")?;
    let path = temp_dir.path().join("sessions.db");
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true);
    let pool = sqlx::SqlitePool::connect_with(options).await?;
    sqlx::query("CREATE TABLE sessions (id TEXT PRIMARY KEY)")
        .execute(&pool)
        .await?;
    pool.close().await;
    let memory = Arc::new(CachedMemory::new(
        crate::session::RigStoreConfig::new(crate::session::StoreType::Sqlite)
            .with_store_path(&path),
    ));
    let bus = Bus::default();
    let mut rx = bus.lifecycle().subscribe();
    let compaction = CompactionConfig {
        compactor: NuCompactor::new(
            MockCompletionModel::text("summary").erase(),
            Bus::default(),
            rig_memory::CompactionPolicy::default(),
            test_model_spec(),
        ),
        reserve_pct: crate::config::defaults::DEFAULT_RESERVE_PCT,
    };
    let meter = crate::hook::AnchorMeter::default();
    let store_latch: crate::hook::StoreLatch = Default::default();
    let pending = crate::hook::chain::PendingSummary::default();
    let spec = spec_with_input_limit(60_000)?;
    let model = compaction.compactor.model();

    // -- Exec: two decisions over the dead store.
    decide_compaction(
        &Message::user("hi"),
        "conv-1",
        InPlace {
            spec: Some(&spec),
            pending: &pending,
        },
        &CompactionPath::new(memory.as_ref(), &compaction, &meter, &store_latch, &model),
        bus.stream(),
        bus.lifecycle(),
    )
    .await;
    decide_compaction(
        &Message::user("hi"),
        "conv-1",
        InPlace {
            spec: Some(&spec),
            pending: &pending,
        },
        &CompactionPath::new(memory.as_ref(), &compaction, &meter, &store_latch, &model),
        bus.stream(),
        bus.lifecycle(),
    )
    .await;

    // -- Check: one line, carrying the refusal itself.
    let failed = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .map_err(|_| "should receive Failed within timeout")?
        .map_err(|_| "should receive Failed")?;
    let crate::bus::LifecycleEvent::CompactionFailed { message, .. } = failed else {
        return Err(format!("the refusal SHALL be a Failed event, got {failed:?}").into());
    };
    assert!(
        message.contains("table sessions already exists"),
        "the line SHALL carry the migration failure: {message}"
    );
    assert!(
        !message.contains("legacy") && !message.contains("Memory backend error"),
        "the line SHALL carry neither the old era nor the wrapper wording: {message}"
    );
    assert!(
        rx.try_recv().is_err(),
        "a latched store SHALL make no further attempt"
    );
    assert!(
        store_latch
            .lock()
            .map_err(|_| "store latch poisoned")?
            .is_some(),
        "the refusal SHALL latch the store"
    );
    Ok(())
}
