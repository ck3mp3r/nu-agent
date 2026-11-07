//! The turn-usage accumulator: the cells the hook writes as each call
//! completes, which the turn executor reads at the turn's end.

use super::super::*;
use super::*;

/// The hook accumulates the turn's usage publisher-side: the cells carry the
/// call's counters, its total, and its cost flag, so the turn executor can
/// report the sums without re-reading the event stream.
#[tokio::test]
async fn turn_usage_accumulates_the_call_the_hook_observed() -> Result<()> {
    // -- Setup & Fixtures
    let model = catalog_turn("copilot/claude-sonnet-4.6", usage())?;
    let TurnHook {
        hook,
        bus,
        turn_usage,
        ..
    } = turn_hook(&model, pricing_less_summarizer())?;

    // -- Exec
    let _ = drive_turn(hook, &bus, model).await?;
    let totals = turn_usage.totals();

    // -- Check
    assert_eq!(totals.input_tokens, 1_000, "the call's input tokens land");
    assert_eq!(totals.output_tokens, 500, "the call's output tokens land");
    assert_eq!(
        totals.total_tokens,
        Some(1_500),
        "the wire's total lands as the call's total"
    );
    assert!(
        totals.cost.is_some_and(|cost| cost > 0.0),
        "the priced call's cost lands; got {:?}",
        totals.cost
    );
    assert!(
        totals.cost_complete,
        "a catalog-priced call is a complete cost"
    );
    Ok(())
}

/// A wire that omits its total still contributes input + output to the turn
/// sum: the chain's fallback is what the accumulator receives.
#[tokio::test]
async fn turn_usage_total_uses_the_chain_fallback_when_the_wire_omits_it() -> Result<()> {
    // -- Setup & Fixtures: no `total_tokens` on the wire's usage.
    let usage = ModelUsage::new().input_tokens(40).output_tokens(2);
    let model = catalog_turn("copilot/claude-sonnet-4.6", usage)?;
    let TurnHook {
        hook,
        bus,
        turn_usage,
        ..
    } = turn_hook(&model, pricing_less_summarizer())?;

    // -- Exec
    let _ = drive_turn(hook, &bus, model).await?;
    let totals = turn_usage.totals();

    // -- Check
    assert_eq!(
        totals.total_tokens,
        Some(42),
        "the accumulator receives the chain's input + output fallback"
    );
    Ok(())
}
