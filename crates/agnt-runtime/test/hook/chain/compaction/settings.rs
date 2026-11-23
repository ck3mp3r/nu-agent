//! The `[compaction]` settings the decision reads: rig's `enabled` and
//! `reserve_tokens`, and what each does to the automatic trigger.

use super::super::*;
use super::*;

/// WHEN `[compaction]` sets `reserve_tokens` higher, THEN THE decision SHALL fire
/// on a request the default reserve leaves under the threshold: the setting, not a
/// derived share of the window, is what the trigger measures against.
#[tokio::test]
async fn decide_compaction_reserve_tokens_moves_the_threshold() -> Result<()> {
    // -- Setup & Fixtures: ~26k tokens over three messages against a 60k limit.
    // The default 16_384-token reserve leaves 43_616 usable, so the request fits;
    // 40_000 tokens of reserve leave 20_000, so the same request is over.
    let history: Vec<Message> = (0..3)
        .map(|i| Message::user(format!("message {i} {}", "z".repeat(30_000))))
        .collect();

    // -- Exec: once at the default settings, once with 40_000 tokens reserved.
    let default_fx = decide_fixture()?;
    seed_rig(&default_fx, &history).await?;
    decide_compaction(
        &Message::user("hi"),
        "conv-1",
        Some(&default_fx.spec),
        &default_fx.pending,
        &CompactionPath::new(
            default_fx.memory.as_ref(),
            &default_fx.compaction,
            &default_fx.meter,
            &Default::default(),
            &default_fx.model,
        ),
        default_fx.bus.stream(),
        default_fx.bus.lifecycle(),
    )
    .await;

    let mut high_fx = decide_fixture()?;
    high_fx.compaction = NuCompactor::new(
        high_fx.summary_model.clone().erase(),
        high_fx.bus.clone(),
        rig_memory::CompactionSettings {
            reserve_tokens: 40_000,
            // Keeps only the newest message, as the fixture's own settings do.
            keep_recent_tokens: 1,
            ..rig_memory::CompactionSettings::default()
        },
        test_model_spec(),
    );
    seed_rig(&high_fx, &history).await?;
    decide_compaction(
        &Message::user("hi"),
        "conv-1",
        Some(&high_fx.spec),
        &high_fx.pending,
        &CompactionPath::new(
            high_fx.memory.as_ref(),
            &high_fx.compaction,
            &high_fx.meter,
            &Default::default(),
            &high_fx.model,
        ),
        high_fx.bus.stream(),
        high_fx.bus.lifecycle(),
    )
    .await;

    // -- Check
    assert!(
        default_fx.pending.take_settled().is_none(),
        "the default reserve must leave the 26k-token request under the threshold"
    );
    assert!(
        high_fx.pending.take_settled().is_some(),
        "40_000 reserved tokens must put the same request over the threshold"
    );
    Ok(())
}

/// WHEN `[compaction]` sets `enabled = false`, THEN THE decision SHALL publish
/// nothing and stash nothing, however far over the window the request is.
#[tokio::test]
async fn decide_compaction_disabled_settings_publish_nothing() -> Result<()> {
    // -- Setup & Fixtures: a conversation far over any limit a spec would name.
    let fx = decide_fixture()?;
    let compaction = NuCompactor::new(
        fx.summary_model.clone().erase(),
        fx.bus.clone(),
        rig_memory::CompactionSettings {
            enabled: false,
            ..rig_memory::CompactionSettings::default()
        },
        test_model_spec(),
    );
    seed_rig(&fx, &[Message::user("x".repeat(400_000))]).await?;
    let mut rx = fx.bus.control().subscribe();

    // -- Exec
    decide_compaction(
        &Message::user("hi"),
        "conv-1",
        Some(&fx.spec),
        &fx.pending,
        &CompactionPath::new(
            fx.memory.as_ref(),
            &compaction,
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
        "a disabled compaction SHALL publish nothing"
    );
    assert!(
        fx.pending.take_settled().is_none(),
        "a disabled compaction SHALL stash no summary"
    );
    Ok(())
}

/// WHEN a completion settled as a context overflow, THEN THE next decision SHALL
/// schedule the side-call although the estimate is under the threshold — the
/// previous summary may have gone in after its prompt — and SHALL consume the
/// verdict, so one refusal schedules one compaction.
#[tokio::test]
async fn decide_compaction_overflow_verdict_bypasses_the_threshold() -> Result<()> {
    // -- Setup & Fixtures: a short conversation, far under the 60k limit less the
    // 16_384-token reserve, so only the verdict can fire it. The under-threshold
    // case without a verdict is `decide_compaction_under_threshold_publishes_nothing`.
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
        "an overflow still leaves this turn's request unpatched, got {action:?}"
    );
    let settled = fx
        .pending
        .take_settled()
        .ok_or("the overflow verdict must schedule the summary")?;
    assert!(
        settled.reply().contains("summary"),
        "the side-call's answer must land as the summary, got {:?}",
        settled.reply()
    );
    assert!(
        !fx.compaction.take_overflowed(),
        "the decision must consume the verdict"
    );
    Ok(())
}
