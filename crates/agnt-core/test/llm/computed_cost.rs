use super::*;

use rig::DynModel;
use rig::catalog::Catalog;
use rig::completion::Usage;
use rig::operation::Completion;
use rig::test_utils::MockCompletionModel;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// A wire answering as the catalog's `reference`, so its own `spec()` is the
/// catalog-bound, priced one.
fn catalog_model(reference: &str) -> Result<DynModel<Completion>> {
    let spec = Catalog::builtin()
        .resolve(reference)
        .map_err(|e| format!("{reference} must be in the built-in catalog: {e}"))?
        .shared();
    Ok(MockCompletionModel::text("x").priced_as(spec).erase())
}

/// A usage that reports both counters, so the catalog can price it.
fn usage() -> Usage {
    Usage::new()
        .input_tokens(1_000)
        .output_tokens(500)
        .total_tokens(1_500)
}

/// The priced total and completeness, or an error naming the skip.
fn priced(outcome: PricedOutcome) -> Result<(f64, bool)> {
    match outcome {
        PricedOutcome::Priced { total, complete } => Ok((total, complete)),
        PricedOutcome::Skipped(reason) => {
            Err(format!("expected a price, got a skip: {reason}").into())
        }
    }
}

/// The skip reason, or an error naming the price.
fn skipped(outcome: PricedOutcome) -> Result<PricingSkip> {
    match outcome {
        PricedOutcome::Skipped(reason) => Ok(reason),
        PricedOutcome::Priced { total, .. } => {
            Err(format!("expected a skip, got a price of {total}").into())
        }
    }
}

// endregion: --- Test Support

/// A row the catalog prices yields the row's rates times the counted tokens,
/// so a copilot reply the driver leaves unpriced still shows a cost.
#[test]
fn test_llm_computed_cost_prices_a_listed_row() -> Result<()> {
    // -- Setup & Fixtures
    let model = catalog_model("copilot/claude-sonnet-4.6")?;
    let usage = usage();

    // -- Exec
    let (total, complete) = priced(computed_cost(&model, &usage))?;

    // -- Check
    let expected = (1_000.0 * 3.0 + 500.0 * 15.0) / 1_000_000.0;
    assert!(
        (total - expected).abs() < 1e-12,
        "the total must be the row's rates times the tokens, got {total}"
    );
    assert!(
        complete,
        "a row that lists every rate prices the whole usage"
    );
    Ok(())
}

/// A row without a price stays tokens-only, and the skip names that reason.
#[test]
fn test_llm_computed_cost_unpriced_row_skips_with_no_pricing() -> Result<()> {
    // -- Setup & Fixtures
    let model = catalog_model("copilot/gpt-4o")?;
    let usage = usage();

    // -- Exec & Check
    assert_eq!(
        skipped(computed_cost(&model, &usage))?,
        PricingSkip::NoPricing,
        "a row the catalog does not price must skip with NoPricing"
    );
    Ok(())
}

/// The compaction spec is built from our config and carries no pricing, so a
/// wire bound to it prices nothing. Guards the fallback against reading that
/// spec instead of the wire's own catalog-bound one.
#[test]
fn test_llm_computed_cost_compaction_spec_is_pricing_less() -> Result<()> {
    // -- Setup & Fixtures
    let spec = crate::conversation::compaction::compactor::resolve_model_spec(
        "openai",
        "gpt-4o",
        Some(200_000),
        Some(1_024),
    )
    .ok_or("openai must resolve to a rig provider id")?;
    let model = MockCompletionModel::text("x").priced_as(spec).erase();
    let usage = usage();

    // -- Exec & Check
    assert_eq!(
        skipped(computed_cost(&model, &usage))?,
        PricingSkip::NoPricing,
        "the compaction spec carries no pricing, so it must skip with NoPricing"
    );
    Ok(())
}

/// A usage that reports no output tokens cannot be priced, so the call stays
/// tokens-only rather than charging a partial figure.
#[test]
fn test_llm_computed_cost_missing_output_tokens_skips_with_missing_counters() -> Result<()> {
    // -- Setup & Fixtures
    let model = catalog_model("copilot/claude-sonnet-4.6")?;
    let usage = Usage::new().input_tokens(1_000).total_tokens(1_000);

    // -- Exec & Check
    assert_eq!(
        skipped(computed_cost(&model, &usage))?,
        PricingSkip::MissingCounters,
        "a usage without output tokens must skip with MissingCounters"
    );
    Ok(())
}

/// A usage that reports no input tokens cannot be priced either.
#[test]
fn test_llm_computed_cost_missing_input_tokens_skips_with_missing_counters() -> Result<()> {
    // -- Setup & Fixtures
    let model = catalog_model("copilot/claude-sonnet-4.6")?;
    let usage = Usage::new().output_tokens(500).total_tokens(500);

    // -- Exec & Check
    assert_eq!(
        skipped(computed_cost(&model, &usage))?,
        PricingSkip::MissingCounters,
        "a usage without input tokens must skip with MissingCounters"
    );
    Ok(())
}

/// A wire the catalog does not list has no row to price from, so the call
/// stays tokens-only, and the skip names that reason.
#[test]
fn test_llm_computed_cost_unlisted_wire_skips_with_no_spec() -> Result<()> {
    // -- Setup & Fixtures: the default mock wire addresses no model.
    let model: DynModel<Completion> = MockCompletionModel::text("x").erase();
    let usage = usage();

    // -- Exec & Check
    assert_eq!(
        skipped(computed_cost(&model, &usage))?,
        PricingSkip::NoSpec,
        "a wire the catalog does not list must skip with NoSpec"
    );
    Ok(())
}

/// A cache part with tokens but no listed rate leaves the total a lower bound,
/// so the label keeps its `+` marker.
#[test]
fn test_llm_computed_cost_unlisted_cache_rate_is_incomplete() -> Result<()> {
    // -- Setup & Fixtures: the row lists no cache-write rate.
    let model = catalog_model("copilot/gemini-3.5-flash")?;
    let usage = usage().cache_creation_input_tokens(100);

    // -- Exec
    let (total, complete) = priced(computed_cost(&model, &usage))?;

    // -- Check
    let expected = (900.0 * 1.5 + 500.0 * 9.0) / 1_000_000.0;
    assert!(
        (total - expected).abs() < 1e-12,
        "the total must leave out the unlisted cache part, got {total}"
    );
    assert!(
        !complete,
        "a cache part with tokens but no listed rate makes the cost a lower bound"
    );
    Ok(())
}

/// The fallback's correctness depends on each provider's wire reporting the
/// catalog's vendor name, so the name resolves the row the provider serves.
#[test]
fn test_llm_computed_cost_wire_name_maps_to_catalog_vendor() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        ("copilot/claude-sonnet-4.6", "copilot"),
        ("ollama/gemma4:31b", "ollama"),
        ("openai/gpt-4.1", "openai"),
        ("anthropic/claude-sonnet-4-6", "anthropic"),
    ];

    // -- Exec & Check
    for (reference, vendor) in cases {
        let model = catalog_model(reference)?;
        assert_eq!(
            model.name(),
            vendor,
            "{reference} must report its catalog vendor"
        );
        let spec = model
            .spec()
            .ok_or_else(|| format!("{reference} must resolve a spec"))?;
        assert!(
            spec.pricing.is_some(),
            "{reference} must list a price for the fallback to compute one"
        );
    }
    Ok(())
}
