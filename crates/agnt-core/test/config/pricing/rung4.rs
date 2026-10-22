//! Rung 4 (config pricing) and the sweep over every fixture row.

use super::*;

// region:    --- rung 4 (config pricing)

#[test]
fn test_pricing_config_price_prices_the_residual() -> Result<()> {
    // -- Setup & Fixtures: no vendor prices llama3.3, so the config price is
    // the only rung that can price it.
    let cache = fixture_cache();
    let view = PricingView::new(&cache);
    let config_pricing = config_price(0.0, 0.0);

    // -- Exec
    let resolved = view.pricing_for("ollama", "llama3.3", Some(&config_pricing));

    // -- Check
    assert_eq!(resolved.rung, LadderRung::Config);
    assert_eq!(rates_of(resolved.pricing), Some((0.0, 0.0)));
    Ok(())
}

#[test]
fn test_pricing_config_price_wins_over_a_priced_catalog_row() -> Result<()> {
    // -- Setup & Fixtures: the catalog prices the model, and the user declared
    // a different price. The user stated what they pay, so it wins.
    let cache = fixture_cache();
    let view = PricingView::new(&cache);
    let config_pricing = config_price(99.0, 99.0);

    // -- Exec
    let resolved = view.pricing_for("github-copilot", "gpt-4o", Some(&config_pricing));

    // -- Check
    assert_eq!(resolved.rung, LadderRung::Config);
    assert_eq!(
        rates_of(resolved.pricing),
        Some((99.0, 99.0)),
        "the configured price must win over the catalog row"
    );
    Ok(())
}

#[test]
fn test_pricing_config_price_without_cache_rates_leaves_them_unset() -> Result<()> {
    // -- Setup & Fixtures: a config price that lists no cache rates.
    let cache = fixture_cache();
    let view = PricingView::new(&cache);
    let config_pricing = config_price(1.0, 2.0);

    // -- Exec
    let resolved = view.pricing_for("ollama", "llama3.3", Some(&config_pricing));

    // -- Check
    let pricing = resolved
        .pricing
        .ok_or("the config price must price the model")?;
    assert_eq!(pricing.cache_read, None);
    assert_eq!(pricing.cache_write, None);
    Ok(())
}

#[test]
fn test_pricing_from_config_without_a_price_is_the_residual() -> Result<()> {
    // -- Exec
    let resolved = ResolvedPricing::from_config(None);

    // -- Check
    assert_eq!(resolved.rung, LadderRung::Residual);
    assert_eq!(resolved.pricing, None);
    Ok(())
}

#[test]
fn test_pricing_from_config_with_a_price_is_the_config_rung() -> Result<()> {
    // -- Setup & Fixtures
    let config_pricing = config_price(1.0, 2.0);

    // -- Exec
    let resolved = ResolvedPricing::from_config(Some(&config_pricing));

    // -- Check
    assert_eq!(resolved.rung, LadderRung::Config);
    assert_eq!(rates_of(resolved.pricing), Some((1.0, 2.0)));
    Ok(())
}

// endregion: --- rung 4 (config pricing)

// region:    --- sweep

#[test]
fn test_pricing_sweep_every_row_resolves_its_rung() -> Result<()> {
    // -- Setup & Fixtures
    let cache = fixture_cache();
    let view = PricingView::new(&cache);
    // (provider key, model id, expected rung, expected rates)
    let expected: [Expected<'_>; 5] = [
        ("github-copilot", "gpt-4o", LadderRung::Residual, None),
        (
            "github-copilot",
            "claude-sonnet-4.6",
            LadderRung::Catalog,
            Some((3.0, 15.0)),
        ),
        ("openai", "gpt-4o", LadderRung::Catalog, Some((2.5, 10.0))),
        (
            "anthropic",
            "claude-sonnet-4-6",
            LadderRung::Catalog,
            Some((3.0, 15.0)),
        ),
        ("ollama-cloud", "llama3.3", LadderRung::Residual, None),
    ];

    // -- Exec & Check
    let mut swept = 0;
    let mut residual: Vec<String> = Vec::new();
    for (provider, provider_spec) in &cache.providers {
        for model_id in provider_spec.models.keys() {
            let resolved = view.pricing_for(provider, model_id, None);
            let (_, _, rung, rates) = expected
                .iter()
                .find(|(listed_provider, listed_model, _, _)| {
                    listed_provider == provider && listed_model == model_id
                })
                .ok_or_else(|| format!("{provider}/{model_id} has no expected rung"))?;
            assert_eq!(&resolved.rung, rung, "{provider}/{model_id}");
            assert_eq!(rates_of(resolved.pricing), *rates, "{provider}/{model_id}");
            if resolved.rung == LadderRung::Residual {
                residual.push(format!("{provider}/{model_id}"));
            }
            swept += 1;
        }
    }
    assert_eq!(
        swept,
        expected.len(),
        "the sweep must visit every fixture row"
    );
    residual.sort();
    assert_eq!(
        residual,
        vec![
            "github-copilot/gpt-4o".to_string(),
            "ollama-cloud/llama3.3".to_string(),
        ],
        "the residual set is the documented class: an unpriced id no other vendor's row prices"
    );
    Ok(())
}

#[test]
fn test_pricing_sweep_config_price_wins_over_every_row() -> Result<()> {
    // -- Setup & Fixtures: the same fixture, with a configured price. The
    // configured price wins over every catalog row, so every model resolves
    // to it.
    let cache = fixture_cache();
    let view = PricingView::new(&cache);
    let config_pricing = config_price(0.5, 1.5);
    // (provider key, model id, expected rung, expected rates)
    let expected: [Expected<'_>; 5] = [
        (
            "github-copilot",
            "gpt-4o",
            LadderRung::Config,
            Some((0.5, 1.5)),
        ),
        (
            "github-copilot",
            "claude-sonnet-4.6",
            LadderRung::Config,
            Some((0.5, 1.5)),
        ),
        ("openai", "gpt-4o", LadderRung::Config, Some((0.5, 1.5))),
        (
            "anthropic",
            "claude-sonnet-4-6",
            LadderRung::Config,
            Some((0.5, 1.5)),
        ),
        (
            "ollama-cloud",
            "llama3.3",
            LadderRung::Config,
            Some((0.5, 1.5)),
        ),
    ];

    // -- Exec & Check
    let mut swept = 0;
    let mut residual: Vec<String> = Vec::new();
    for (provider, provider_spec) in &cache.providers {
        for model_id in provider_spec.models.keys() {
            let resolved = view.pricing_for(provider, model_id, Some(&config_pricing));
            let (_, _, rung, rates) = expected
                .iter()
                .find(|(listed_provider, listed_model, _, _)| {
                    listed_provider == provider && listed_model == model_id
                })
                .ok_or_else(|| format!("{provider}/{model_id} has no expected rung"))?;
            assert_eq!(&resolved.rung, rung, "{provider}/{model_id}");
            assert_eq!(rates_of(resolved.pricing), *rates, "{provider}/{model_id}");
            if resolved.rung == LadderRung::Residual {
                residual.push(format!("{provider}/{model_id}"));
            }
            swept += 1;
        }
    }
    assert_eq!(
        swept,
        expected.len(),
        "the sweep must visit every fixture row"
    );
    assert!(
        residual.is_empty(),
        "a configured price must leave no residual, got {residual:?}"
    );
    Ok(())
}

// endregion: --- sweep
