//! The auto ladder's end: the summary a side-call writes between the runs, and
//! the store the next run loads.

use super::super::*;
use super::*;

/// WHEN the decision crosses the threshold, THEN THE decision SHALL take the
/// side-call: the request goes unpatched, no worker compaction is scheduled, and
/// the summary waits for the executor's between-runs splice — which is what the
/// next run loads.
#[tokio::test]
async fn side_call_lands_between_the_runs_and_the_next_load_sees_it() -> Result<()> {
    // -- Setup & Fixtures
    // The end-to-end proof that the Auto worker path is closed: the decide step
    // reads the live rig conversation and asks for the summary on its own. Four
    // messages of 30k chars each are over a 5k limit.
    let fx = decide_fixture()?;
    let spec = spec_with_input_limit(5_000)?;
    let pending = crate::hook::chain::PendingSummary::default();
    let history: Vec<Message> = (0..4)
        .map(|i| Message::user(format!("message {i} {}", "z".repeat(30_000))))
        .collect();
    seed_rig(&fx, &history).await?;
    let mut control_rx = fx.bus.control().subscribe();

    // -- Exec: the decide step takes the side-call.
    let action = decide_compaction(
        &Message::user("hi"),
        "conv-1",
        Some(&spec),
        &pending,
        &CompactionPath::new(
            fx.memory.as_ref(),
            &fx.compaction,
            &fx.meter,
            &Default::default(),
            &fx.model,
        ),
        fx.bus.stream(),
        fx.bus.lifecycle(),
    )
    .await;

    // -- Check: the request is unpatched and nothing reaches the worker.
    assert!(
        matches!(action, rig::agent::CompletionCallAction::Continue),
        "a side-call leaves this turn's request alone, got {action:?}"
    );
    assert!(
        matches!(
            control_rx.try_recv(),
            Err(crate::bus::TryReceiveError::Empty)
        ),
        "auto compaction SHALL NOT schedule a worker compaction"
    );
    let settled = pending
        .take_settled()
        .ok_or("the side-call's summary must wait for the executor")?;

    // -- Exec: the executor's between-runs splice lands the summary.
    let store = fx
        .memory
        .rig_store("conv-1")
        .await
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    crate::conversation::compaction::supersede::land(
        settled,
        store,
        "conv-1".to_string(),
        fx.bus.stream(),
        fx.bus.lifecycle(),
    )
    .await;

    // -- Check: the next run loads a conversation led by the summary.
    let store = fx
        .memory
        .rig_store("conv-1")
        .await
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
    assert!(
        stored.messages().len() < history.len(),
        "the splice must replace the messages the side-call summarized"
    );
    Ok(())
}

#[tokio::test]
async fn run_compaction_empty_history_emits_completed_with_empty_summary() -> Result<()> {
    // -- Setup & Fixtures
    let fx = decide_fixture()?;
    let mut rx = fx.bus.lifecycle().subscribe();

    // -- Exec
    let result = run_compaction(
        "conv-1",
        &CompactionPath::new(
            fx.memory.as_ref(),
            &fx.compaction,
            &fx.meter,
            &Default::default(),
            &fx.model,
        ),
        CompactionSource::Manual,
        fx.bus.stream(),
        fx.bus.lifecycle(),
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
            crate::bus::LifecycleEvent::CompactionCompleted {
                source,
                summary,
            } if *source == CompactionSource::Manual && summary.is_empty()
        ),
        "an empty conversation must emit Completed with empty summary, got {completed:?}"
    );
    Ok(())
}

/// WHEN `run_compaction` opens the store, THE latch SHALL clear.
#[tokio::test]
async fn successful_run_compaction_clears_the_latch() -> Result<()> {
    // -- Setup & Fixtures: a latched store that is now reachable.
    let memory = Arc::new(CachedMemory::new(
        crate::session::RigStoreConfig::in_memory(),
    ));
    let bus = Bus::default();
    let compaction = NuCompactor::new(
        MockCompletionModel::text("summary").erase(),
        Bus::default(),
        rig_memory::CompactionSettings::default(),
        test_model_spec(),
    );
    let meter = crate::hook::AnchorMeter::default();
    let store_latch: crate::hook::StoreLatch = Default::default();
    *store_latch.lock().map_err(|_| "store latch poisoned")? = Some("dead".to_string());
    let model = compaction.model();

    // -- Exec
    let _ = run_compaction(
        "conv-1",
        &CompactionPath::new(memory.as_ref(), &compaction, &meter, &store_latch, &model),
        CompactionSource::Manual,
        bus.stream(),
        bus.lifecycle(),
    )
    .await;

    // -- Check
    assert!(
        store_latch
            .lock()
            .map_err(|_| "store latch poisoned")?
            .is_none(),
        "a store that opens SHALL clear the latch"
    );
    Ok(())
}

/// WHEN a manual compaction runs while a provider refusal is on record, THEN THE
/// run SHALL consume the verdict, so the next decision — whose estimate is under
/// the threshold — plans nothing: one refusal asks for one compaction, and the
/// one that ran is that compaction.
#[tokio::test]
async fn run_compaction_consumes_the_overflow_verdict() -> Result<()> {
    // -- Setup & Fixtures: a settled overflow on a conversation far under the 60k
    // limit less the 16_384-token reserve, so only the verdict could fire either
    // path.
    let fx = decide_fixture()?;
    seed_rig(
        &fx,
        &[
            Message::user("turn one"),
            Message::assistant("turn one reply"),
        ],
    )
    .await?;
    fx.compaction.mark_overflowed();

    // -- Exec: the user's `/compact`.
    let _ = run_compaction(
        "conv-1",
        &CompactionPath::new(
            fx.memory.as_ref(),
            &fx.compaction,
            &fx.meter,
            &Default::default(),
            &fx.model,
        ),
        CompactionSource::Manual,
        fx.bus.stream(),
        fx.bus.lifecycle(),
    )
    .await;

    // -- Check: the verdict is spent, and the next decision plans nothing.
    assert!(
        !fx.compaction.take_overflowed(),
        "a manual compaction must consume the verdict"
    );
    let action = decide_compaction(
        &Message::user("hi"),
        "conv-1",
        Some(&fx.spec),
        &fx.pending,
        &CompactionPath::new(
            fx.memory.as_ref(),
            &fx.compaction,
            &fx.meter,
            &Default::default(),
            &fx.model,
        ),
        fx.bus.stream(),
        fx.bus.lifecycle(),
    )
    .await;
    assert!(
        matches!(action, rig::agent::CompletionCallAction::Continue),
        "the next decision leaves the request alone, got {action:?}"
    );
    assert!(
        fx.pending.take_settled().is_none(),
        "a spent verdict must leave the under-threshold request alone"
    );
    Ok(())
}
