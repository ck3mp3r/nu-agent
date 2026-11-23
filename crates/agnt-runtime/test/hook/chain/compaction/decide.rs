//! The threshold decision: which turns schedule the side-call summary, and which
//! publish nothing.

use super::super::*;
use super::*;

/// WHEN the metered request is over the model's input limit less rig's
/// `reserve_tokens`, THE begin_completion SHALL schedule the side-call summary:
/// the request goes unpatched, the summary waits for the executor's between-runs
/// splice, and a Started row is published.
#[tokio::test]
async fn decide_compaction_over_threshold_schedules_the_side_call() -> Result<()> {
    // -- Setup & Fixtures: ~57k tokens of conversation against a 60k limit, whose
    // rig-default reserve of 16_384 tokens leaves 43_616 usable, so the request is
    // over it.
    let fx = decide_fixture()?;
    let history: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("message {i} {}", "q".repeat(40_000))))
        .collect();
    seed_rig(&fx, &history).await?;
    let mut lifecycle_rx = fx.bus.lifecycle().subscribe();
    let mut compaction_rx = fx.bus.control().subscribe();

    // -- Exec
    let action = decide_compaction(
        &Message::user("current user prompt"),
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

    // -- Check: the turn's own request goes unpatched, no worker compaction is
    // scheduled, and the side-call's summary waits for the executor.
    assert!(
        matches!(action, rig::agent::CompletionCallAction::Continue),
        "an over-threshold turn must not patch its request, got {action:?}"
    );
    assert!(
        matches!(
            compaction_rx.try_recv(),
            Err(crate::bus::TryReceiveError::Empty)
        ),
        "auto compaction SHALL NOT schedule a worker compaction"
    );
    let settled = fx
        .pending
        .take_settled()
        .ok_or("the side-call's summary must wait for the executor")?;
    assert_eq!(
        settled.reply(),
        "summary",
        "the side-call's answer must land as the summary"
    );
    assert_eq!(
        fx.turn_model.requests().len(),
        1,
        "the side-call asks the turn's own model exactly once"
    );
    let started = tokio::time::timeout(std::time::Duration::from_secs(5), lifecycle_rx.recv())
        .await
        .map_err(|_| "should receive CompactionStarted within timeout")?
        .map_err(|_| "should receive CompactionStarted")?;
    assert!(
        matches!(
            &started,
            crate::bus::LifecycleEvent::CompactionStarted { source } if *source == CompactionSource::Auto
        ),
        "the side-call must announce a started compaction, got {started:?}"
    );
    Ok(())
}

/// WHEN the conversation is dominated by one enormous last message, THEN THE
/// begin_completion SHALL take the side-call: the request goes unpatched, no
/// worker compaction is scheduled, and the summary waits for the executor.
#[tokio::test]
async fn decide_compaction_with_a_dominant_last_message_takes_the_side_call() -> Result<()> {
    // -- Setup & Fixtures: three tiny messages and one enormous last message, so
    // the kept window lands on that message alone.
    let fx = decide_fixture()?;
    let mut history: Vec<Message> = (0..3).map(|i| Message::user(format!("tiny {i}"))).collect();
    history.push(Message::user("w".repeat(400_000)));
    seed_rig(&fx, &history).await?;
    let mut compaction_rx = fx.bus.control().subscribe();

    // -- Exec
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

    // -- Check
    assert!(
        matches!(action, rig::agent::CompletionCallAction::Continue),
        "an over-threshold turn must not patch its request, got {action:?}"
    );
    assert!(
        matches!(
            compaction_rx.try_recv(),
            Err(crate::bus::TryReceiveError::Empty)
        ),
        "auto compaction SHALL NOT schedule a worker compaction"
    );
    let settled = fx
        .pending
        .take_settled()
        .ok_or("the side-call's summary must wait for the executor")?;
    assert_eq!(
        settled.reply(),
        "summary",
        "the side-call's answer must land as the summary"
    );
    Ok(())
}

/// WHEN the model's input limit is tight, THEN THE begin_completion SHALL take the
/// side-call and ask the turn's own model exactly once.
#[tokio::test]
async fn decide_compaction_with_a_tight_input_limit_takes_the_side_call() -> Result<()> {
    // -- Setup & Fixtures: a 5k limit, under rig's 16_384-token reserve, so the
    // trigger leaves no room at all and any context is over it. The huge message
    // sits in the middle, so the cut is a real one and the side-call plans a
    // request of its own.
    let fx = decide_fixture()?;
    let spec = spec_with_input_limit(5_000)?;
    seed_rig(
        &fx,
        &[
            Message::user("an older question"),
            Message::user("s".repeat(40_000)),
            Message::user("a recent question"),
        ],
    )
    .await?;
    let mut compaction_rx = fx.bus.control().subscribe();

    // -- Exec
    let action = decide_compaction(
        &Message::user("hi"),
        "conv-1",
        Some(&spec),
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

    // -- Check
    assert!(
        matches!(action, rig::agent::CompletionCallAction::Continue),
        "an over-threshold turn must not patch its request, got {action:?}"
    );
    assert!(
        matches!(
            compaction_rx.try_recv(),
            Err(crate::bus::TryReceiveError::Empty)
        ),
        "auto compaction SHALL NOT schedule a worker compaction"
    );
    assert!(
        fx.pending.take_settled().is_some(),
        "the side-call's summary must wait for the executor"
    );
    assert_eq!(
        fx.turn_model.requests().len(),
        1,
        "the side-call asks the turn's own model exactly once"
    );
    Ok(())
}

/// WHEN the metered request is over the model's input limit less rig's
/// `reserve_tokens`, THE decision SHALL complete the compaction on this turn by a
/// side-call and SHALL NOT schedule an `Auto` worker compaction.
#[tokio::test]
async fn decide_compaction_over_threshold_never_schedules_an_auto_worker() -> Result<()> {
    // -- Setup & Fixtures: ~57k tokens of conversation against a 60k limit, whose
    // rig-default reserve of 16_384 tokens leaves 43_616 usable, so the request is
    // over it.
    let fx = decide_fixture()?;
    let mut history: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("user message {i} ").repeat(2)))
        .collect();
    // One enormous last message, so the conversation is dominated by its tail.
    history.push(Message::user("x".repeat(400_000)));
    seed_rig(&fx, &history).await?;
    let mut compaction_rx = fx.bus.control().subscribe();

    // -- Exec
    decide_compaction(
        &Message::user("current user prompt"),
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

    // -- Check: the decision took the turn, and the control channel stayed silent.
    assert!(
        matches!(
            compaction_rx.try_recv(),
            Err(crate::bus::TryReceiveError::Empty)
        ),
        "auto compaction SHALL NOT publish CompactionRequested"
    );
    assert!(
        fx.pending.take_settled().is_some(),
        "an over-threshold turn must decide the compaction itself"
    );
    assert_eq!(
        fx.turn_model.requests().len(),
        1,
        "the side-call asks the turn's own model exactly once"
    );
    assert_eq!(
        fx.summary_model.requests().len(),
        0,
        "the side-call must not use the compactor's own model"
    );
    Ok(())
}

/// WHEN the metered request leaves the reserve free, THE decision SHALL publish
/// nothing.
#[tokio::test]
async fn decide_compaction_under_threshold_publishes_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let fx = decide_fixture()?;
    seed_rig(&fx, &[Message::user("short context message")]).await?;
    let mut rx = fx.bus.control().subscribe();

    // -- Exec
    decide_compaction(
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

    // -- Check
    assert!(
        matches!(rx.try_recv(), Err(crate::bus::TryReceiveError::Empty)),
        "under-threshold turn must not publish any compaction event"
    );
    Ok(())
}

/// WHEN the active model's input limit is unknown, THE decision SHALL meter and
/// publish nothing: rig answers `false` without a limit, so the turn continues.
#[tokio::test]
async fn decide_compaction_without_a_spec_publishes_nothing() -> Result<()> {
    // -- Setup & Fixtures: a conversation far over any limit a spec would name.
    let fx = decide_fixture()?;
    seed_rig(&fx, &[Message::user("x".repeat(400_000))]).await?;
    let mut rx = fx.bus.control().subscribe();

    // -- Exec
    decide_compaction(
        &Message::user("hi"),
        "conv-1",
        None,
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

    // -- Check
    assert!(
        matches!(rx.try_recv(), Err(crate::bus::TryReceiveError::Empty)),
        "a model with no known input limit must not fire Requested"
    );
    let anchor = fx.meter.anchor();
    assert!(
        anchor.is_none(),
        "a decision that cannot meter must record no request"
    );
    Ok(())
}

#[tokio::test]
async fn decide_compaction_counts_a_turn_saved_after_the_first_read() -> Result<()> {
    // -- Setup & Fixtures
    // The discriminating test for currency: turn 1 reads the small conversation,
    // turn 2 grows the rig conversation over the limit. The decision must read
    // the CURRENT rig conversation; on a stale first read it would under-read.
    let fx = decide_fixture()?;
    seed_rig(
        &fx,
        &[
            Message::user("turn one"),
            Message::assistant("turn one reply"),
        ],
    )
    .await?;

    // Turn 1: read the small conversation.
    let store = fx
        .memory
        .rig_store("conv-1")
        .await
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let first_read = rig_memory::load(&*store, "conv-1")?;
    assert_eq!(
        first_read.messages().len(),
        2,
        "turn 1 reads the small conversation"
    );

    // Turn 2: the turn's save grows the rig conversation far past the limit.
    seed_rig(&fx, &[Message::user("y".repeat(400_000))]).await?;

    // -- Exec
    decide_compaction(
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

    // -- Check
    assert!(
        fx.pending.take_settled().is_some(),
        "the post-turn-2 conversation is over the limit, so the decision must act; \
         a stale first read would not"
    );
    Ok(())
}
