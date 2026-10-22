use super::*;

/// Seed the fixture's rig-memory store with `messages`.
async fn seed_rig(fx: &DecideFixture, messages: &[Message]) -> Result<()> {
    let store = fx
        .memory
        .rig_store("conv-1")
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let mut conversation = rig_memory::load(&*store, "conv-1")?;
    conversation.extend(messages.iter().cloned());
    rig_memory::save(&*store, "conv-1", &mut conversation)?;
    Ok(())
}

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
    let mut rx = bus.compaction().subscribe();
    let compaction = CompactionConfig {
        compactor: NuCompactor::new(
            MockCompletionModel::text("summary").erase(),
            Bus::default(),
            rig_memory::CompactionPolicy::default(),
            test_model_spec(),
        ),
        params: CompactionParams::default(),
        threshold_tokens: Some(100),
    };
    let last_total_tokens = Arc::new(Mutex::new(None));

    // -- Exec
    decide_compaction(
        &Message::user("hi"),
        "conv-1",
        memory.as_ref(),
        &compaction,
        &last_total_tokens,
        &bus,
    )
    .await;

    // -- Check
    let failed = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .map_err(|_| "should receive Failed within timeout")?
        .map_err(|_| "should receive Failed")?;
    assert!(
        matches!(&failed, CompactionEvent::Failed { .. }),
        "an unusable rig store must be surfaced in a Failed event, got {failed:?}"
    );
    Ok(())
}

#[tokio::test]
async fn over_threshold_fires_requested_and_does_not_compact_synchronously() -> Result<()> {
    // -- Setup & Fixtures
    // The rig store holds a conversation far above the token threshold.
    // `decide_compaction` must fire a `CompactionEvent::Requested { source: "auto" }`
    // on the bus and NOT invoke the summarizer LLM synchronously (the
    // orchestrator runs compaction).
    let fx = decide_fixture()?;
    let history: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("user message {i} ").repeat(400)))
        .collect();
    seed_rig(&fx, &history).await?;
    let mut compaction_rx = fx.bus.compaction().subscribe();
    let compaction = CompactionConfig {
        threshold_tokens: Some(100),
        ..fx.compaction.clone()
    };
    let prompt = Message::user("current user prompt");

    // -- Exec
    decide_compaction(
        &prompt,
        "conv-1",
        fx.memory.as_ref(),
        &compaction,
        &fx.last_total_tokens,
        &fx.bus,
    )
    .await;

    // -- Check
    let requested = compaction_rx
        .recv()
        .await
        .map_err(|_| "should receive a Requested event")?;
    assert!(
        matches!(
            &requested,
            crate::bus::CompactionEvent::Requested { source } if source == "auto"
        ),
        "over-threshold turn must fire Requested {{ source: \"auto\" }}, got {requested:?}"
    );
    assert_eq!(
        fx.summary_model.requests().len(),
        0,
        "decide_compaction must NOT invoke the summarizer LLM synchronously"
    );
    Ok(())
}

#[tokio::test]
async fn decide_compaction_real_tokens_over_threshold_fires() -> Result<()> {
    // -- Setup & Fixtures
    let fx = decide_fixture()?;
    seed_rig(&fx, &[Message::user("short context message")]).await?;
    // Real count (90000) plus a small prompt estimate (> 80_000) → fires.
    *fx.last_total_tokens.lock().unwrap() = Some(90_000);
    let mut rx = fx.bus.compaction().subscribe();

    // -- Exec
    decide_compaction(
        &Message::user("hi"),
        "conv-1",
        fx.memory.as_ref(),
        &fx.compaction,
        &fx.last_total_tokens,
        &fx.bus,
    )
    .await;

    // -- Check
    let requested = rx.recv().await.map_err(|_| "should receive Requested")?;
    assert!(
        matches!(&requested, CompactionEvent::Requested { source } if source == "auto"),
        "real total_tokens over threshold must fire Requested, got {requested:?}"
    );
    Ok(())
}

#[tokio::test]
async fn decide_compaction_real_tokens_under_threshold_does_not_fire() -> Result<()> {
    // -- Setup & Fixtures
    let fx = decide_fixture()?;
    seed_rig(&fx, &[Message::user("short context message")]).await?;
    // Real count (70000) plus a small prompt estimate (< 80_000) → does not fire.
    *fx.last_total_tokens.lock().unwrap() = Some(70_000);
    let mut rx = fx.bus.compaction().subscribe();

    // -- Exec
    decide_compaction(
        &Message::user("hi"),
        "conv-1",
        fx.memory.as_ref(),
        &fx.compaction,
        &fx.last_total_tokens,
        &fx.bus,
    )
    .await;

    // -- Check
    assert!(
        matches!(rx.try_recv(), Err(crate::bus::TryRecvError::Empty)),
        "under-threshold turn must not publish any compaction event"
    );
    Ok(())
}

#[tokio::test]
async fn decide_compaction_no_real_tokens_falls_back_to_conversation_estimate() -> Result<()> {
    // -- Setup & Fixtures
    let fx = decide_fixture()?;
    // `last_total_tokens` stays `None` (first turn). The conversation is sized so
    // its chars/4 estimate alone is over the threshold → fires via the fallback.
    seed_rig(&fx, &[Message::user("x".repeat(400_000))]).await?;
    let mut rx = fx.bus.compaction().subscribe();

    // -- Exec
    decide_compaction(
        &Message::user("hi"),
        "conv-1",
        fx.memory.as_ref(),
        &fx.compaction,
        &fx.last_total_tokens,
        &fx.bus,
    )
    .await;

    // -- Check
    let requested = rx.recv().await.map_err(|_| "should receive Requested")?;
    assert!(
        matches!(&requested, CompactionEvent::Requested { source } if source == "auto"),
        "fallback estimate over threshold must fire Requested, got {requested:?}"
    );
    Ok(())
}

#[tokio::test]
async fn decide_compaction_counts_a_turn_saved_after_the_cache_was_populated() -> Result<()> {
    // -- Setup & Fixtures
    // The discriminating test for threshold currency: turn 1 populates the
    // first-read cache, turn 2 grows the rig conversation past the threshold.
    // The decision must read the CURRENT rig conversation; on the stale turn-1
    // cache it would under-read and not fire.
    let fx = decide_fixture()?;
    let compaction = CompactionConfig {
        threshold_tokens: Some(50_000),
        ..fx.compaction.clone()
    };
    seed_rig(
        &fx,
        &[
            Message::user("turn one"),
            Message::assistant("turn one reply"),
        ],
    )
    .await?;

    // Turn 1: populate the cache with the small conversation.
    let cached = fx
        .memory
        .load("conv-1")
        .await
        .map_err(|e| format!("cache load should succeed: {e:?}"))?;
    assert_eq!(cached.len(), 2, "turn 1 caches the small conversation");

    // Turn 2: the turn's save grows the rig conversation far past the threshold.
    seed_rig(&fx, &[Message::user("y".repeat(400_000))]).await?;

    let mut rx = fx.bus.compaction().subscribe();

    // -- Exec
    decide_compaction(
        &Message::user("hi"),
        "conv-1",
        fx.memory.as_ref(),
        &compaction,
        &fx.last_total_tokens,
        &fx.bus,
    )
    .await;

    // -- Check
    let requested = rx.recv().await.map_err(|_| "should receive Requested")?;
    assert!(
        matches!(&requested, CompactionEvent::Requested { source } if source == "auto"),
        "the post-turn-2 conversation is over the threshold, so it must fire; \
         the stale turn-1 cache would not, got {requested:?}"
    );
    Ok(())
}

#[tokio::test]
async fn auto_compaction_splices_and_the_next_load_sees_it() -> Result<()> {
    // -- Setup & Fixtures
    // The end-to-end proof that the inert window is closed: the decide step reads
    // the live rig conversation, schedules the compaction, and the splice it
    // makes is what a run loads next.
    let fx = decide_fixture()?;
    let compaction = CompactionConfig {
        threshold_tokens: Some(100),
        ..fx.compaction.clone()
    };
    // Four messages of 30k chars each: over the policy's 20k-token keep_recent,
    // so the automatic cut has something to summarize.
    let history: Vec<Message> = (0..4)
        .map(|i| Message::user(format!("message {i} {}", "z".repeat(30_000))))
        .collect();
    seed_rig(&fx, &history).await?;
    let mut rx = fx.bus.compaction().subscribe();

    // -- Exec: the decide step schedules the compaction.
    decide_compaction(
        &Message::user("hi"),
        "conv-1",
        fx.memory.as_ref(),
        &compaction,
        &fx.last_total_tokens,
        &fx.bus,
    )
    .await;
    let requested = rx.recv().await.map_err(|_| "should receive Requested")?;
    assert!(
        matches!(&requested, CompactionEvent::Requested { source } if source == "auto"),
        "the over-threshold turn must schedule the compaction, got {requested:?}"
    );

    // -- Exec: the orchestrator runs it.
    let patched = run_compaction(
        "conv-1",
        fx.memory.as_ref(),
        &compaction,
        "auto",
        &fx.last_total_tokens,
        &fx.bus,
    )
    .await
    .ok_or("the scheduled compaction should have run")?;

    // -- Check: the splice is what the store holds, so the next run loads it.
    let store = fx
        .memory
        .rig_store("conv-1")
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let stored = rig_memory::load(&*store, "conv-1")?;
    let first = stored
        .entries()
        .next()
        .ok_or("the compacted conversation is non-empty")?;
    assert_eq!(
        first.author,
        Some(&rig_memory::summary_author()),
        "the next load must lead with the rig.compaction summary"
    );
    assert_eq!(
        patched.len(),
        stored.messages().len(),
        "the returned post-splice history must match what the store holds"
    );
    Ok(())
}

#[tokio::test]
async fn run_compaction_empty_history_emits_completed_with_empty_summary() -> Result<()> {
    // -- Setup & Fixtures
    let fx = decide_fixture()?;
    let mut rx = fx.bus.compaction().subscribe();

    // -- Exec
    let result = run_compaction(
        "conv-1",
        fx.memory.as_ref(),
        &fx.compaction,
        "slash",
        &fx.last_total_tokens,
        &fx.bus,
    )
    .await;

    // -- Check
    assert!(
        result.is_none(),
        "an empty conversation must return None (nothing to compact)"
    );
    let completed = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .map_err(|_| "should receive Completed within timeout")?
        .map_err(|_| "should receive Completed")?;
    assert!(
        matches!(
            &completed,
            CompactionEvent::Completed {
                source,
                summary_preview,
                summary_body,
            } if source == "slash"
                && summary_preview.is_empty()
                && summary_body.is_empty()
        ),
        "an empty conversation must emit Completed with empty summary, got {completed:?}"
    );
    Ok(())
}
