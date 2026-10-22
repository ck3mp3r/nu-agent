//! The model-identity invariant: pricing resolves by model id, never by the
//! vendor whose wire serves the model.

use super::*;

/// The pricing module's own source, so the guard reads the real signature.
const PRICING_SOURCE: &str = include_str!("../../../src/config/pricing.rs");

// region:    --- cross-vendor resolution

/// A model id priced only under a section that differs from the wire vendor
/// does not resolve: there is no cross-vendor borrowing.
///
/// The config provider here is `openai` — the vendor the wire reports — and
/// its own row lists the id with no cost. Only the `ollama-cloud` section
/// prices it. The block declares no `catalog`, so it reaches no other section
/// and the model stays tokens-only.
#[test]
fn test_pricing_cross_vendor_priced_id_does_not_resolve_without_a_declared_catalog() -> Result<()> {
    // -- Setup & Fixtures: the wire vendor's own row is unpriced; a different
    // vendor's section prices the same id.
    let cache = cache_of(&[
        ("openai", "deepseek-v4.1-flash", None),
        ("ollama-cloud", "deepseek-v4.1-flash", Some((0.15, 0.60))),
    ]);
    let view = PricingView::new(&cache);

    // -- Exec
    let resolved = view.pricing_for("openai", "deepseek-v4.1-flash", None);

    // -- Check
    assert_eq!(
        resolved.rung,
        LadderRung::Residual,
        "no other section's row may price the model"
    );
    assert_eq!(
        resolved.pricing, None,
        "the model must stay tokens-only without a declared catalog"
    );
    Ok(())
}

/// The same fixture, read through the reseller's own section: the ladder
/// reaches the `ollama-cloud` section by identity, so the catalog row answers.
#[test]
fn test_pricing_reseller_config_name_reaches_its_own_cache_section() -> Result<()> {
    // -- Setup & Fixtures
    let cache = cache_of(&[
        ("openai", "deepseek-v4.1-flash", None),
        ("ollama-cloud", "deepseek-v4.1-flash", Some((0.15, 0.60))),
    ]);
    let view = PricingView::new(&cache);

    // -- Exec
    let resolved = view.pricing_for("ollama-cloud", "deepseek-v4.1-flash", None);

    // -- Check
    assert_eq!(resolved.rung, LadderRung::Catalog);
    assert_eq!(rates_of(resolved.pricing), Some((0.15, 0.60)));
    Ok(())
}

// endregion: --- cross-vendor resolution

// region:    --- no wire vendor in the ladder

/// The ladder takes the config provider name and the model id. It takes no
/// wire vendor, so no caller can scope a price lookup to the vendor that
/// happens to serve the model.
#[test]
fn test_pricing_pricing_for_takes_no_wire_vendor() -> Result<()> {
    // -- Setup & Fixtures: the `pricing_for` parameter list, as written.
    let signature = PRICING_SOURCE
        .split("pub fn pricing_for(")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .ok_or("the pricing module must declare `pricing_for`")?;
    let parameters: Vec<&str> = signature
        .split(',')
        .map(str::trim)
        .filter(|parameter| !parameter.is_empty())
        .collect();

    // -- Exec & Check
    assert_eq!(
        parameters,
        [
            "&self",
            "provider: &str",
            "model: &str",
            "config_pricing: Option<&ModelCost>",
        ],
        "`pricing_for` must take the config provider, the model id, and the configured price"
    );
    assert!(
        !PRICING_SOURCE.contains("vendor:"),
        "no function in the pricing module may take a wire vendor"
    );
    Ok(())
}

// endregion: --- no wire vendor in the ladder
