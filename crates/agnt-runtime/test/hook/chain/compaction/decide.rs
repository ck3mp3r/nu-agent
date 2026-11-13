//! The in-place decision: which turns patch their request, which take the
//! interim worker scheduling, and which publish nothing.

use super::super::*;
use super::*;

use crate::hook::chain::in_place::InPlace;

/// WHEN the metered request is over the model's input limit less the policy's
/// reserve, and the conversation has a part worth summarizing inside the room the
/// model leaves, THE begin_completion SHALL patch the call in place: the kept
/// messages plus one user message carrying the summarizer's instruction, the
/// measured output budget, a stashed plan for the reply, and a Started row.
#[tokio::test]
async fn decide_compaction_over_threshold_patches_in_place() -> Result<()> {
    // -- Setup & Fixtures: ~50k tokens of conversation against a 60k limit whose
    // reserve (20 percent of the limit) leaves 48_000 usable. The kept window
    // is a quarter of the limit, so
    // the summarized part is most of the conversation (worth a model call) and
    // the room left is far above the output floor.
    let fx = decide_fixture()?;
    let history: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("message {i} {}", "q".repeat(40_000))))
        .collect();
    seed_rig(&fx, &history).await?;
    let mut lifecycle_rx = fx.bus.lifecycle().subscribe();

    // -- Exec
    let action = decide_compaction(
        &Message::user("current user prompt"),
        "conv-1",
        InPlace {
            spec: Some(&fx.spec),
            pending: &fx.pending,
        },
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

    // -- Check: a patch whose history is the kept messages plus the instruction.
    let rig::agent::CompletionCallAction::Patch(patch) = action else {
        return Err(format!(
            "an over-threshold turn with room to summarize must patch, got {action:?}"
        )
        .into());
    };
    let sent = patch
        .history
        .ok_or("the patch replaces the request's history")?;
    let instruction = sent
        .last()
        .ok_or("the patch sends at least the instruction")?;
    let rig::message::Message::User { content } = &**instruction else {
        return Err("the last message the patch sends is the instruction".into());
    };
    let text = content
        .iter()
        .find_map(|item| match item {
            rig::message::UserContent::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .ok_or("the instruction is a user text message")?;
    assert!(
        text.starts_with(fx.compaction.policy().summarizer.prompts.in_place.as_ref()),
        "the instruction must be the summarizer's in-place prompt, got: {text}"
    );
    assert!(
        sent.len() < history.len(),
        "the patch must send fewer messages than the conversation holds"
    );
    let budget = patch
        .max_tokens
        .ok_or("the patch asks for an output budget")?;
    assert!(
        budget >= 4_096,
        "the budget must clear the output floor, got {budget}"
    );
    assert!(
        fx.pending.is_pending(),
        "the decision must stash the plan the reply answers"
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
        "the patch must announce a started compaction, got {started:?}"
    );
    Ok(())
}

/// WHEN the part being summarized is too small a share of the conversation, THEN
/// THE begin_completion SHALL take the side-call: the request goes unpatched, no
/// worker compaction is scheduled, and the summary waits for the executor.
#[tokio::test]
async fn decide_compaction_not_worth_summarizing_takes_the_side_call() -> Result<()> {
    // -- Setup & Fixtures: three tiny messages and one enormous last message, so
    // the kept window lands on that message alone and the part being summarized
    // is a rounding error against the whole.
    let fx = decide_fixture()?;
    let mut history: Vec<Message> = (0..3).map(|i| Message::user(format!("tiny {i}"))).collect();
    history.push(Message::user("w".repeat(400_000)));
    seed_rig(&fx, &history).await?;
    let mut compaction_rx = fx.bus.control().subscribe();

    // -- Exec
    let action = decide_compaction(
        &Message::user("hi"),
        "conv-1",
        InPlace {
            spec: Some(&fx.spec),
            pending: &fx.pending,
        },
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
        "a summary that is not worth its call must not patch, got {action:?}"
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
    assert!(
        settled.summary().is_some(),
        "the side-call's answer must be a usable summary, got {:?}",
        settled.rejection()
    );
    Ok(())
}

/// WHEN the kept history leaves less room than the summary needs, THEN THE
/// begin_completion SHALL take the side-call: the in-place request is impossible
/// under that model's limit, and plain messages leave nothing to clear.
#[tokio::test]
async fn decide_compaction_without_room_for_the_summary_takes_the_side_call() -> Result<()> {
    // -- Setup & Fixtures: a 5k limit whose quarter (the kept window) leaves less
    // than the output floor below itself, and a conversation over the threshold the
    // 20 percent reserve leaves (4_000 usable). The huge message sits in the
    // middle, so the cut is a real one — the kept window opens on it — and only the
    // output budget makes the in-place request impossible.
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
        InPlace {
            spec: Some(&spec),
            pending: &fx.pending,
        },
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
        "a request with no room for the summary must not patch, got {action:?}"
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

/// WHEN the metered request is over the model's input limit less the policy's
/// reserve, THE decision SHALL complete the compaction on this turn — in place,
/// cleared, or by a side-call — and SHALL NOT schedule an `Auto` worker
/// compaction.
#[tokio::test]
async fn decide_compaction_over_threshold_never_schedules_an_auto_worker() -> Result<()> {
    // -- Setup & Fixtures: ~50k tokens of conversation against a 60k limit whose
    // reserve (20 percent of the limit) leaves 48_000 usable, so the request is
    // over it.
    let fx = decide_fixture()?;
    let mut history: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("user message {i} ").repeat(2)))
        .collect();
    // One enormous last message: the kept window lands on it alone, so the part
    // being summarized is too small a share of the conversation for an in-place
    // summary and the side-call answers instead.
    history.push(Message::user("x".repeat(400_000)));
    seed_rig(&fx, &history).await?;
    let mut compaction_rx = fx.bus.control().subscribe();

    // -- Exec
    decide_compaction(
        &Message::user("current user prompt"),
        "conv-1",
        InPlace {
            spec: Some(&fx.spec),
            pending: &fx.pending,
        },
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

    // -- Check: the ladder took the turn, and the control channel stayed silent.
    assert!(
        matches!(
            compaction_rx.try_recv(),
            Err(crate::bus::TryReceiveError::Empty)
        ),
        "auto compaction SHALL NOT publish CompactionRequested"
    );
    assert!(
        fx.pending.is_pending() || fx.pending.take_settled().is_some(),
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
        InPlace {
            spec: Some(&fx.spec),
            pending: &fx.pending,
        },
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
        InPlace {
            spec: None,
            pending: &fx.pending,
        },
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
    let (anchor, sent) = fx.meter.metering();
    assert!(
        anchor.is_none() && sent.is_none(),
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
        InPlace {
            spec: Some(&fx.spec),
            pending: &fx.pending,
        },
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
        fx.pending.is_pending() || fx.pending.take_settled().is_some(),
        "the post-turn-2 conversation is over the limit, so the decision must act; \
         a stale first read would not"
    );
    Ok(())
}
