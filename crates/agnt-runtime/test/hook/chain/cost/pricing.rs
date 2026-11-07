//! What a completion costs: the wire's own figure, the catalog's rates, and
//! the completeness of the sum.

use super::super::*;
use super::*;

/// Copilot is one of rig's unpriced providers, so the driver leaves
/// `usage.cost` None while the catalog prices the row. The hook must compute
/// the cost from the wire's own catalog-bound spec.
#[tokio::test]
async fn cost_computed_from_the_wire_spec_when_the_endpoint_reports_none() -> Result<()> {
    // -- Setup & Fixtures
    let model = catalog_turn("copilot/claude-sonnet-4.6", usage())?;

    // -- Exec
    let (cost, cost_complete) = emitted_cost(completed_event(model).await?)?;

    // -- Check
    let total = cost.ok_or("a priced row must yield a computed cost")?;
    let expected = (1_000.0 * 3.0 + 500.0 * 15.0) / 1_000_000.0;
    assert!(
        (total - expected).abs() < 1e-12,
        "the emitted cost must be the row's rates times the tokens, got {total}"
    );
    assert!(
        cost_complete,
        "a row that lists every rate prices the whole usage"
    );
    Ok(())
}

/// A provider the driver prices reports its own cost, and the hook emits that
/// figure rather than recomputing one.
#[tokio::test]
async fn cost_reported_by_the_endpoint_is_emitted_unchanged() -> Result<()> {
    // -- Setup & Fixtures: the reported figure, from a plain call on the same
    // wire.
    let plain = catalog_call("anthropic/claude-sonnet-4-6", usage())?.erase();
    let reported = plain
        .call("run")
        .await
        .map_err(|e| format!("the plain call must succeed: {e}"))?
        .usage
        .cost
        .ok_or("a priced provider's reply must carry a cost")?
        .total;
    let model = catalog_turn("anthropic/claude-sonnet-4-6", usage())?;

    // -- Exec
    let (cost, cost_complete) = emitted_cost(completed_event(model).await?)?;

    // -- Check
    let total = cost.ok_or("a reported cost must be emitted")?;
    assert!(
        (total - reported).abs() < 1e-12,
        "the emitted cost must be the reported one, got {total} against {reported}"
    );
    assert!(cost_complete, "the reported cost covers the whole usage");
    Ok(())
}

/// A row the catalog does not price stays tokens-only, so no figure is
/// invented for it.
#[tokio::test]
async fn cost_stays_tokens_only_for_an_unpriced_row() -> Result<()> {
    // -- Setup & Fixtures
    let model = catalog_turn("copilot/gpt-4o", usage())?;

    // -- Exec
    let (cost, cost_complete) = emitted_cost(completed_event(model).await?)?;

    // -- Check
    assert_eq!(
        cost, None,
        "a row the catalog does not price must stay tokens-only"
    );
    assert!(!cost_complete, "an unpriced call is not a complete cost");
    Ok(())
}

/// A cache part with tokens but no listed rate leaves the total a lower bound,
/// so the emitted cost is marked incomplete.
#[tokio::test]
async fn cost_marks_an_unlisted_cache_rate_incomplete() -> Result<()> {
    // -- Setup & Fixtures: the row lists no cache-write rate.
    let model = catalog_turn(
        "copilot/gemini-3.5-flash",
        usage().cache_creation_input_tokens(100),
    )?;

    // -- Exec
    let (cost, cost_complete) = emitted_cost(completed_event(model).await?)?;

    // -- Check
    let total = cost.ok_or("a priced row must yield a computed cost")?;
    let expected = (900.0 * 1.5 + 500.0 * 9.0) / 1_000_000.0;
    assert!(
        (total - expected).abs() < 1e-12,
        "the emitted cost must leave out the unlisted cache part, got {total}"
    );
    assert!(
        !cost_complete,
        "a cache part with tokens but no listed rate makes the cost a lower bound"
    );
    Ok(())
}

/// A usage that reports both counters but no total is still priced. The
/// ollama-cloud OpenAI-compatible endpoint omits `total_tokens`, and rig passes
/// that absence through verbatim, so the total must never gate pricing.
#[tokio::test]
async fn cost_computed_when_the_usage_reports_no_total_tokens() -> Result<()> {
    // -- Setup & Fixtures: the ollama-cloud shape — input and output reported,
    // total absent. The row lists 3.00 input and 15.00 output per million.
    let model = catalog_turn(
        "copilot/claude-sonnet-4.6",
        ModelUsage::new().input_tokens(1_000).output_tokens(500),
    )?;

    // -- Exec
    let (cost, cost_complete) = emitted_cost(completed_event(model).await?)?;

    // -- Check
    let total = cost.ok_or("a priced row must yield a computed cost")?;
    let expected = (1_000.0 * 3.0 + 500.0 * 15.0) / 1_000_000.0;
    assert!(
        (total - expected).abs() < 1e-12,
        "the emitted cost must be the row's rates times the tokens, got {total}"
    );
    assert!(
        cost_complete,
        "a row that lists every rate prices the whole usage"
    );
    Ok(())
}

/// The compactor's wire is not the turn's: a turn on a priced model is priced
/// at that model's rates, not at the summarizer's, which nothing prices.
#[tokio::test]
async fn cost_priced_with_the_turns_own_model() -> Result<()> {
    // -- Setup & Fixtures: the turn's wire is priced, the summarizer's is not.
    let model = catalog_turn("copilot/claude-sonnet-4.6", usage())?;

    // -- Exec
    let (cost, cost_complete) =
        emitted_cost(completed_event_with(model, pricing_less_summarizer()).await?)?;

    // -- Check
    let total = cost.ok_or("the turn's own priced model must yield a computed cost")?;
    let expected = (1_000.0 * 3.0 + 500.0 * 15.0) / 1_000_000.0;
    assert!(
        (total - expected).abs() < 1e-12,
        "the emitted cost must be the turn's model's rates times the tokens, got {total}"
    );
    assert!(
        cost_complete,
        "a row that lists every rate prices the whole usage"
    );
    Ok(())
}

/// A turn on a model nothing prices records no cost, even when the
/// summarizer's wire is priced — the hook must not invent a cost from it —
/// and the warning names the turn's model, not the summarizer's.
#[tokio::test]
async fn cost_stays_none_for_a_pricing_less_turn_model() -> Result<()> {
    // -- Setup & Fixtures: the turn's row lists no price, the summarizer's
    // does.
    let model = catalog_turn("copilot/gpt-4o", usage())?;
    let summarizer = catalog_turn("copilot/claude-sonnet-4.6", usage())?;
    let TurnHook { hook, bus, .. } = turn_hook(&model, summarizer)?;

    // -- Exec
    // The once-per-model gate is consumed by the read, so read the warning
    // before the turn runs.
    let warning = hook
        .unpriced_warning(PricingSkip::NoPricing)
        .ok_or("the first unpriced call must warn")?;
    let (cost, cost_complete) = emitted_cost(
        drive_turn(hook, &bus, model)
            .await?
            .ok_or("the turn must emit LlmCompleted")?,
    )?;

    // -- Check
    assert_eq!(
        cost, None,
        "a turn on a model nothing prices must stay tokens-only"
    );
    assert!(!cost_complete, "an unpriced call is not a complete cost");
    assert!(
        warning.contains("copilot/gpt-4o"),
        "the warning must name the turn's model, got {warning}"
    );
    Ok(())
}
