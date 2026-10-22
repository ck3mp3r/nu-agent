//! The shipped rungs: the catalog row and the residual.

use super::*;

// region:    --- pricing_for

#[test]
fn test_pricing_pricing_for_own_row_is_the_catalog_rung() -> Result<()> {
    // -- Setup & Fixtures
    let cache = fixture_cache();
    let view = PricingView::new(&cache);

    // -- Exec
    let resolved = view.pricing_for("anthropic", "claude-sonnet-4-6", None);

    // -- Check
    assert_eq!(resolved.rung, LadderRung::Catalog);
    assert_eq!(rates_of(resolved.pricing), Some((3.0, 15.0)));
    Ok(())
}

#[test]
fn test_pricing_own_row_wins_over_another_vendors_row() -> Result<()> {
    // -- Setup & Fixtures: both vendors price the id, at different rates.
    let cache = cache_of(&[
        ("github-copilot", "gpt-4o", Some((9.0, 9.0))),
        ("openai", "gpt-4o", Some((2.5, 10.0))),
    ]);
    let view = PricingView::new(&cache);

    // -- Exec
    let resolved = view.pricing_for("openai", "gpt-4o", None);

    // -- Check
    assert_eq!(resolved.rung, LadderRung::Catalog);
    assert_eq!(
        rates_of(resolved.pricing),
        Some((2.5, 10.0)),
        "the own row must win, never another vendor's row"
    );
    Ok(())
}

#[test]
fn test_pricing_unpriced_row_is_the_residual() -> Result<()> {
    // -- Setup & Fixtures: copilot lists gpt-4o with no cost, and openai
    // prices the same id. No borrowing happens, so the row stays unpriced.
    let cache = fixture_cache();
    let view = PricingView::new(&cache);

    // -- Exec
    let resolved = view.pricing_for("github-copilot", "gpt-4o", None);

    // -- Check
    assert_eq!(resolved.rung, LadderRung::Residual);
    assert_eq!(
        resolved.pricing, None,
        "an unpriced row must not borrow another vendor's rates"
    );
    Ok(())
}

#[test]
fn test_pricing_unknown_model_is_the_residual() -> Result<()> {
    // -- Setup & Fixtures
    let cache = fixture_cache();
    let view = PricingView::new(&cache);

    // -- Exec
    let resolved = view.pricing_for("openai", "no-such-model", None);

    // -- Check
    assert_eq!(resolved.rung, LadderRung::Residual);
    assert_eq!(resolved.pricing, None);
    Ok(())
}

#[test]
fn test_pricing_section_key_reaches_its_cache_section() -> Result<()> {
    // -- Setup & Fixtures: the ladder takes the resolved section key, so a
    // block that declares `catalog = "github-copilot"` passes that key.
    let cache = fixture_cache();
    let view = PricingView::new(&cache);

    // -- Exec
    let resolved = view.pricing_for("github-copilot", "claude-sonnet-4.6", None);

    // -- Check
    assert_eq!(resolved.rung, LadderRung::Catalog);
    assert_eq!(rates_of(resolved.pricing), Some((3.0, 15.0)));
    Ok(())
}

#[test]
fn test_pricing_unaliased_name_reaches_no_section() -> Result<()> {
    // -- Setup & Fixtures: no name mapping exists, so a block named `copilot`
    // reaches no section unless it declares one.
    let cache = fixture_cache();
    let view = PricingView::new(&cache);

    // -- Exec & Check
    assert!(
        view.cache_spec("copilot", "claude-sonnet-4.6").is_none(),
        "an undeclared name must not reach another section's row"
    );
    assert!(
        view.cache_spec("github-copilot", "claude-sonnet-4.6")
            .is_some(),
        "the declared section key must reach its row"
    );
    Ok(())
}

#[test]
fn test_pricing_cache_fields_are_carried_onto_the_pricing() -> Result<()> {
    // -- Setup & Fixtures
    let mut cache = cache_of(&[("anthropic", "claude-sonnet-4-6", Some((3.0, 15.0)))]);
    let Some(provider) = cache.providers.get_mut("anthropic") else {
        return Err("the fixture must list anthropic".into());
    };
    let Some(spec) = provider.models.get_mut("claude-sonnet-4-6") else {
        return Err("the fixture must list the model".into());
    };
    spec.cost = Some(ModelCost {
        input: 3.0,
        output: 15.0,
        cache_read: Some(0.3),
        cache_write: Some(3.75),
    });
    let view = PricingView::new(&cache);

    // -- Exec
    let resolved = view.pricing_for("anthropic", "claude-sonnet-4-6", None);

    // -- Check
    let pricing = resolved.pricing.ok_or("a priced row must resolve prices")?;
    assert_eq!(pricing.cache_read, Some(0.3));
    assert_eq!(pricing.cache_write, Some(3.75));
    Ok(())
}

// endregion: --- pricing_for
