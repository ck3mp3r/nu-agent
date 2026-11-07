//! `TurnUsage` — the per-turn accumulator of LLM usage the hook chain writes
//! and the turn executor reads when it publishes the turn-completed event.

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_turn_usage_sums_tokens_and_cost_across_calls() -> Result<()> {
    // -- Setup & Fixtures
    let usage = TurnUsage::default();

    // -- Exec
    usage.add_call(10, 5, Some(15), Some(0.1), true);
    usage.add_call(20, 7, Some(27), Some(0.2), true);
    let totals = usage.totals();

    // -- Check
    assert_eq!(totals.input_tokens, 30, "input tokens sum across calls");
    assert_eq!(totals.output_tokens, 12, "output tokens sum across calls");
    assert_eq!(totals.total_tokens, Some(42), "totals sum across calls");
    assert!(
        totals.cost.is_some_and(|cost| (cost - 0.3).abs() < 1e-12),
        "costs sum across calls; got {:?}",
        totals.cost
    );
    assert!(
        totals.cost_complete,
        "every call reported a complete cost, so the sum is complete"
    );
    Ok(())
}

#[test]
fn test_turn_usage_total_uses_the_per_call_fallback() -> Result<()> {
    // -- Setup & Fixtures
    let usage = TurnUsage::default();

    // -- Exec
    // The first call's wire omitted its total, so the chain passed input +
    // output; the second reported one.
    usage.add_call(10, 5, Some(15), None, false);
    usage.add_call(40, 2, Some(42), None, false);
    let totals = usage.totals();

    // -- Check
    assert_eq!(
        totals.total_tokens,
        Some(57),
        "each call contributes its fallback total"
    );
    Ok(())
}

#[test]
fn test_turn_usage_total_is_null_when_every_call_omits_it() -> Result<()> {
    // -- Setup & Fixtures
    let usage = TurnUsage::default();

    // -- Exec
    usage.add_call(10, 0, None, None, false);
    usage.add_call(20, 0, None, None, false);
    let totals = usage.totals();

    // -- Check
    assert_eq!(
        totals.total_tokens, None,
        "no call reported a total, so the turn has none"
    );
    assert_eq!(
        totals.input_tokens, 30,
        "the counters still sum when no total is derivable"
    );
    Ok(())
}

#[test]
fn test_turn_usage_cost_is_null_when_no_call_reports_one() -> Result<()> {
    // -- Setup & Fixtures
    let usage = TurnUsage::default();

    // -- Exec
    usage.add_call(10, 5, Some(15), None, false);
    usage.add_call(20, 7, Some(27), None, false);
    let totals = usage.totals();

    // -- Check
    assert_eq!(
        totals.cost, None,
        "an unpriced turn reports no cost rather than zero"
    );
    Ok(())
}

#[test]
fn test_turn_usage_cost_complete_is_the_and_of_the_call_flags() -> Result<()> {
    // -- Setup & Fixtures
    let usage = TurnUsage::default();

    // -- Exec
    usage.add_call(10, 5, Some(15), Some(0.1), true);
    usage.add_call(20, 7, Some(27), Some(0.2), false);
    let totals = usage.totals();

    // -- Check
    assert!(
        !totals.cost_complete,
        "one incomplete component makes the sum a lower bound"
    );
    Ok(())
}

#[test]
fn test_turn_usage_of_no_calls_is_empty() -> Result<()> {
    // -- Setup & Fixtures
    let usage = TurnUsage::default();

    // -- Exec
    let totals = usage.totals();

    // -- Check
    assert_eq!(totals.input_tokens, 0, "a turn with no calls spent nothing");
    assert_eq!(totals.output_tokens, 0);
    assert_eq!(totals.total_tokens, None, "no call reported a total");
    assert_eq!(totals.cost, None, "no call reported a cost");
    assert!(
        totals.cost_complete,
        "the AND of no flags is vacuously true; cost is null in this case"
    );
    Ok(())
}

#[test]
fn test_turn_usage_totals_are_a_snapshot() -> Result<()> {
    // -- Setup & Fixtures
    let usage = TurnUsage::default();
    usage.add_call(10, 5, Some(15), None, false);

    // -- Exec
    let snapshot = usage.totals();
    usage.add_call(20, 7, Some(27), None, false);

    // -- Check
    assert_eq!(
        snapshot.input_tokens, 10,
        "a read must not see calls that land after it"
    );
    assert_eq!(usage.totals().input_tokens, 30);
    Ok(())
}

#[test]
fn test_turn_usage_reset_zeroes_the_cells_in_place() -> Result<()> {
    // -- Setup & Fixtures
    let usage = TurnUsage::default();
    usage.add_call(10, 5, Some(15), Some(0.1), false);
    // A clone taken before the reset must see the reset: the cells are shared.
    let shared = usage.clone();

    // -- Exec
    usage.reset();
    let totals = shared.totals();

    // -- Check
    assert_eq!(totals.input_tokens, 0, "a reset clears the input sum");
    assert_eq!(totals.output_tokens, 0, "a reset clears the output sum");
    assert_eq!(totals.total_tokens, None, "a reset clears the total sum");
    assert_eq!(totals.cost, None, "a reset clears the cost sum");
    assert!(
        totals.cost_complete,
        "a reset restores the AND identity, so the next call decides the flag"
    );
    Ok(())
}

#[test]
fn test_turn_usage_reset_lets_the_next_call_start_the_sum() -> Result<()> {
    // -- Setup & Fixtures
    let usage = TurnUsage::default();
    usage.add_call(10, 5, Some(15), Some(0.1), false);

    // -- Exec
    usage.reset();
    usage.add_call(40, 2, Some(42), Some(0.2), true);
    let totals = usage.totals();

    // -- Check
    assert_eq!(totals.input_tokens, 40, "only the post-reset call counts");
    assert_eq!(totals.total_tokens, Some(42));
    assert!(
        totals.cost_complete,
        "the post-reset call's flag decides the sum, not the pre-reset one"
    );
    Ok(())
}

#[test]
fn test_turn_usage_totals_default_is_the_empty_sum() -> Result<()> {
    // -- Setup & Fixtures
    let empty = TurnUsageTotals::default();

    // -- Exec & Check
    assert_eq!(empty.input_tokens, 0);
    assert_eq!(empty.output_tokens, 0);
    assert_eq!(empty.total_tokens, None);
    assert_eq!(empty.cost, None);
    assert!(
        empty.cost_complete,
        "the empty sum's AND is vacuously true, matching a fresh accumulator"
    );
    assert_eq!(
        empty,
        TurnUsage::default().totals(),
        "the default totals must equal a fresh accumulator's totals"
    );
    Ok(())
}
