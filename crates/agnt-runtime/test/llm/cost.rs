use super::*;

use rig::catalog::ModelFacts;
use rig::completion::Usage;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// A usage that reports both counters, so the catalog can price it.
fn usage() -> Usage {
    Usage::new()
        .input_tokens(1_000)
        .output_tokens(500)
        .total_tokens(1_500)
}

// endregion: --- Test Support

/// The label sums every priced call, so the displayed figure is the session
/// total rather than the last call's.
#[test]
fn test_llm_session_cost_accumulates_priced_calls() -> Result<()> {
    // -- Setup & Fixtures
    let mut cost = SessionCost::default();

    // -- Exec
    cost.record(Some(0.001), true);
    cost.record(Some(0.002), true);

    // -- Check
    assert_eq!(cost.label().as_deref(), Some("$0.003"));
    Ok(())
}

/// A priced call beside an unpriced one keeps the priced total and marks the
/// label a lower bound with the `+` suffix.
#[test]
fn test_llm_session_cost_mixed_priced_and_unpriced_calls() -> Result<()> {
    // -- Setup & Fixtures
    let mut cost = SessionCost::default();

    // -- Exec
    cost.record(Some(0.001), true);
    cost.record(None, false);

    // -- Check
    assert_eq!(cost.label().as_deref(), Some("$0.001+"));
    Ok(())
}

/// A session nothing priced has no label, so the status line shows the token
/// count alone instead of a `$0.000` placeholder.
#[test]
fn test_llm_session_cost_unpriced_calls_have_no_label() -> Result<()> {
    // -- Setup & Fixtures
    let mut cost = SessionCost::default();

    // -- Exec
    cost.record(None, false);
    cost.record(None, false);

    // -- Check
    assert_eq!(cost.label(), None);
    Ok(())
}

/// A price that covers only part of a call still counts toward the total, and
/// marks the label a lower bound. A session whose every price is partial still
/// has a lower bound worth showing, so it gets the `+` label too.
#[test]
fn test_llm_session_cost_incomplete_price_is_a_lower_bound() -> Result<()> {
    // -- Setup & Fixtures
    let mut mixed = SessionCost::default();
    let mut partial_only = SessionCost::default();

    // -- Exec
    mixed.record(Some(0.001), true);
    mixed.record(Some(0.004), false);
    partial_only.record(Some(0.004), false);

    // -- Check
    assert_eq!(mixed.label().as_deref(), Some("$0.005+"));
    assert_eq!(partial_only.label().as_deref(), Some("$0.004+"));
    Ok(())
}

/// A session whose calls were all priced but incomplete sums its lower bounds
/// and marks the label `+`, so a cache-hit-heavy session still shows a price.
#[test]
fn test_llm_session_cost_all_incomplete_sums_lower_bounds() -> Result<()> {
    // -- Setup & Fixtures
    let mut cost = SessionCost::default();

    // -- Exec
    cost.record(Some(0.001), false);
    cost.record(Some(0.002), false);

    // -- Check
    assert_eq!(cost.label().as_deref(), Some("$0.003+"));
    Ok(())
}

/// An all-incomplete session whose summed cost is zero has no lower bound to
/// show, so it stays tokens-only.
#[test]
fn test_llm_session_cost_all_incomplete_zero_cost_has_no_label() -> Result<()> {
    // -- Setup & Fixtures
    let mut cost = SessionCost::default();

    // -- Exec
    cost.record(Some(0.0), false);

    // -- Check
    assert_eq!(cost.label(), None);
    Ok(())
}

/// A session with no recorded call has no label.
#[test]
fn test_llm_session_cost_empty_has_no_label() -> Result<()> {
    // -- Setup & Fixtures
    let cost = SessionCost::default();

    // -- Exec & Check
    assert_eq!(cost.label(), None);
    Ok(())
}

/// A lower bound at or above one dollar formats with cents, matching rig's own
/// formatter.
#[test]
fn test_llm_session_cost_lower_bound_formats_cents_above_a_dollar() -> Result<()> {
    // -- Setup & Fixtures
    let mut cost = SessionCost::default();

    // -- Exec
    cost.record(Some(1.5), false);

    // -- Check
    assert_eq!(cost.label().as_deref(), Some("$1.50+"));
    Ok(())
}

/// The facts a wire built by `XxxConfig::connect` holds price an OpenAI and an
/// Anthropic reply, so `usage.cost` arrives populated without attaching facts
/// at construction. Guards the cost display against a rig upgrade that stops
/// answering from the built-in catalog.
#[test]
fn test_llm_default_facts_price_openai_and_anthropic_usage() -> Result<()> {
    // -- Setup & Fixtures: the facts our provider construction leaves on the
    // wire, and a usage that reports both counters.
    let facts = ModelFacts::default();
    let usage = usage();

    // -- Exec & Check: both priced providers resolve a price.
    for (provider, model) in [("openai", "gpt-4.1"), ("anthropic", "claude-sonnet-4-5")] {
        let spec = facts
            .for_model(provider, model)
            .ok_or_else(|| format!("{provider}/{model} must be in the built-in catalog"))?;
        let pricing = spec
            .pricing
            .as_ref()
            .ok_or_else(|| format!("{provider}/{model} must list a price"))?;
        let cost = pricing
            .cost(&usage)
            .ok_or_else(|| format!("{provider}/{model} must price a usage with both counters"))?;
        assert!(
            cost.total > 0.0,
            "{provider}/{model} must charge a positive total"
        );
        assert!(
            cost.is_complete(),
            "{provider}/{model} must price every part of the usage"
        );
    }
    Ok(())
}
