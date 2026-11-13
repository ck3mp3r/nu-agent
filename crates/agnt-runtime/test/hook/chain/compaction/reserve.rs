//! The `reserve_pct` knob: the room one decision keeps free below the turn's
//! model input limit.

use super::super::*;
use super::*;

use crate::hook::chain::in_place::InPlace;

/// WHEN `reserve_pct` is unset, THEN THE runtime SHALL resolve it to our 20 and
/// THE decision SHALL keep 12_000 tokens free on a 60_000-token limit.
#[test]
fn keep_free_default_is_twenty_percent_of_the_limit() -> Result<()> {
    // -- Setup & Fixtures: the fixture's 60_000-token spec and default knob.
    let fx = decide_fixture()?;

    // -- Exec
    let room = fx.compaction.keep_free(&fx.spec);

    // -- Check
    assert_eq!(
        fx.compaction.reserve_pct,
        crate::config::defaults::DEFAULT_RESERVE_PCT,
        "the fixture carries the resolved default"
    );
    assert_eq!(room, Some(12_000), "20 percent of 60_000 is 12_000");
    Ok(())
}

/// WHEN an explicit percent is set, THEN THE decision SHALL keep that share
/// free: 40 percent of 60_000 is 24_000.
#[test]
fn keep_free_honors_an_explicit_percent() -> Result<()> {
    // -- Setup & Fixtures
    let mut fx = decide_fixture()?;
    fx.compaction.reserve_pct = 40;

    // -- Exec
    let room = fx.compaction.keep_free(&fx.spec);

    // -- Check
    assert_eq!(room, Some(24_000), "40 percent of 60_000 is 24_000");
    Ok(())
}

/// WHEN the percent does not divide the limit evenly, THEN THE derived room
/// SHALL be truncated, not rounded.
#[test]
fn keep_free_truncates_the_percent() -> Result<()> {
    // -- Setup & Fixtures: 20 percent of 60_003 is 12_000.6.
    let fx = decide_fixture()?;
    let spec = spec_with_input_limit(60_003)?;

    // -- Exec
    let room = fx.compaction.keep_free(&spec);

    // -- Check
    assert_eq!(room, Some(12_000), "the derived room is truncated");
    Ok(())
}

/// WHEN the model's input limit is unknown, THEN THE decision SHALL derive no
/// room and the turn SHALL continue unpatched.
#[test]
fn keep_free_without_an_input_limit_is_none() -> Result<()> {
    // -- Setup & Fixtures: a spec that names no window and no input limit.
    let fx = decide_fixture()?;
    let spec = crate::conversation::compaction::compactor::resolve_model_spec(
        "openai", "gpt-4o", None, None, None,
    )
    .ok_or("openai must resolve to a registered provider id")?;

    // -- Exec
    let room = fx.compaction.keep_free(&spec);

    // -- Check
    assert_eq!(room, None, "no limit derives no room");
    Ok(())
}

/// WHEN the block raises `reserve_pct` to 40, THEN THE decision SHALL fire on a
/// request the default 20 percent leaves under the threshold: the knob, not the
/// policy's stored reserve, is what the trigger measures.
#[tokio::test]
async fn decide_compaction_an_explicit_reserve_pct_moves_the_threshold() -> Result<()> {
    // -- Setup & Fixtures: ~40k tokens over four messages against a 60k limit.
    // The default 20 percent keeps 12_000 free and so fires only above 48_000; 40
    // percent keeps 24_000 free and fires above 36_000.
    let history: Vec<Message> = (0..4)
        .map(|i| Message::user(format!("message {i} {}", "z".repeat(40_000))))
        .collect();

    // -- Exec: once at the default, once at 40 percent.
    let default_fx = decide_fixture()?;
    seed_rig(&default_fx, &history).await?;
    let mut default_rx = default_fx.bus.control().subscribe();
    decide_compaction(
        &Message::user("hi"),
        "conv-1",
        InPlace {
            spec: Some(&default_fx.spec),
            pending: &default_fx.pending,
        },
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
    high_fx.compaction.reserve_pct = 40;
    seed_rig(&high_fx, &history).await?;
    let mut high_rx = high_fx.bus.control().subscribe();
    decide_compaction(
        &Message::user("hi"),
        "conv-1",
        InPlace {
            spec: Some(&high_fx.spec),
            pending: &high_fx.pending,
        },
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
        !default_fx.pending.is_pending() && default_fx.pending.take_settled().is_none(),
        "20 percent must leave the 40k-token request under the threshold"
    );
    assert!(
        matches!(
            default_rx.try_recv(),
            Err(crate::bus::TryReceiveError::Empty)
        ),
        "an under-threshold turn publishes nothing"
    );
    assert!(
        high_fx.pending.is_pending() || high_fx.pending.take_settled().is_some(),
        "40 percent must put the same request over the threshold"
    );
    assert!(
        matches!(high_rx.try_recv(), Err(crate::bus::TryReceiveError::Empty)),
        "the decision takes the turn; no worker compaction is scheduled"
    );
    Ok(())
}
