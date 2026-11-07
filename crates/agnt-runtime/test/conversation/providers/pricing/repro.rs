//! The live ollama-cloud repro: a config block named for a reseller, served
//! through a wire whose vendor differs from the block's cache section.

use super::*;

// region:    --- ollama wire

/// The user's live repro: a config provider block named `ollama-cloud` with
/// `provider = "ollama"`, serving `deepseek-v4.1-flash`, whose cache row is
/// priced. The session showed no cost while `agent models list` showed
/// $0.15/$0.60.
///
/// The transport is a recording client rather than a real one, so this stays a
/// unit test; the config resolution and the facts binding are the real ones.
#[test]
fn test_providers_build_dyn_model_prices_the_ollama_cloud_repro() -> Result<()> {
    use crate::config::{ModelConfig, ModelRoleConfig, PluginConfig, ProviderConfig};

    // -- Setup & Fixtures: the config block is literally named `ollama-cloud`.
    let mut plugin_config = PluginConfig::default();
    plugin_config.providers.insert(
        "ollama-cloud".to_string(),
        ProviderConfig {
            provider: Some("ollama".to_string()),
            models: HashMap::from([("deepseek-v4.1-flash".to_string(), ModelConfig::default())]),
            ..ProviderConfig::default()
        },
    );
    let role_config = ModelRoleConfig {
        model: "ollama-cloud/deepseek-v4.1-flash".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("the repro config must resolve: {e:?}"))?;
    assert_eq!(config.provider, "ollama-cloud");
    assert_eq!(config.provider_impl.as_deref(), Some("ollama"));

    let cache = cache_of(&[("ollama-cloud", "deepseek-v4.1-flash", Some((0.15, 0.60)))]);
    let view = PricingView::new(&cache);
    let client = ollama_client();

    // -- Exec
    let model = client.build_dyn_model(
        &config.model,
        &config.provider,
        Some(&view),
        config.pricing.as_ref(),
    )?;

    // -- Check
    let (total, complete) = priced(computed_cost(&model, &priced_usage()))?;
    let expected = (1_000.0 * 0.15 + 500.0 * 0.60) / 1_000_000.0;
    assert!(
        (total - expected).abs() < 1e-12,
        "the repro model must price at the cache row's rates, got {total}"
    );
    assert!(complete, "the row lists every rate the usage needs");
    Ok(())
}

// endregion: --- ollama wire

// region:    --- openai wire

/// The user's exact live repro: a config block named `ollama-cloud` that
/// serves through the **openai** wire (`provider = "openai"`,
/// `base_url = "https://ollama.com/v1"`), for `deepseek-v4.1-flash`.
///
/// The cache fixture is the discriminating part. The `openai` section exists
/// and prices other ids, but it does not list this one; only the
/// `ollama-cloud` section prices it. A wire-vendor-scoped lookup would find
/// nothing under `openai` and leave the session tokens-only, while
/// `agent models list` showed $0.15/$0.60. The model-id ladder finds the
/// `ollama-cloud` row.
#[test]
fn test_providers_build_dyn_model_prices_the_ollama_cloud_openai_wire_repro() -> Result<()> {
    use crate::config::{ModelConfig, ModelRoleConfig, PluginConfig, ProviderConfig};

    // -- Setup & Fixtures: the config block is literally named `ollama-cloud`
    // and serves through the openai wire.
    let mut plugin_config = PluginConfig::default();
    plugin_config.providers.insert(
        "ollama-cloud".to_string(),
        ProviderConfig {
            provider: Some("openai".to_string()),
            base_url: Some("https://ollama.com/v1".to_string()),
            api_key: Some("sk-ollama-test".to_string()),
            models: HashMap::from([("deepseek-v4.1-flash".to_string(), ModelConfig::default())]),
            ..ProviderConfig::default()
        },
    );
    let role_config = ModelRoleConfig {
        model: "ollama-cloud/deepseek-v4.1-flash".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("the repro config must resolve: {e:?}"))?;
    assert_eq!(config.provider, "ollama-cloud");
    assert_eq!(config.provider_impl.as_deref(), Some("openai"));
    assert_eq!(config.base_url.as_deref(), Some("https://ollama.com/v1"));

    let cache = cache_of(&[
        ("openai", "gpt-4o", Some((2.5, 10.0))),
        ("ollama-cloud", "deepseek-v4.1-flash", Some((0.15, 0.60))),
    ]);
    let view = PricingView::new(&cache);
    let client = openai_completions_client();

    // -- Exec
    let model = client.build_dyn_model(
        &config.model,
        &config.provider,
        Some(&view),
        config.pricing.as_ref(),
    )?;

    // -- Check
    assert_eq!(
        model.name(),
        "openai",
        "the wire must report the openai vendor"
    );
    let (total, complete) = priced(computed_cost(&model, &priced_usage()))?;
    let expected = (1_000.0 * 0.15 + 500.0 * 0.60) / 1_000_000.0;
    assert!(
        (total - expected).abs() < 1e-12,
        "the repro model must price at the ollama-cloud row's rates, got {total}"
    );
    assert!(complete, "the row lists every rate the usage needs");
    Ok(())
}

// endregion: --- openai wire
