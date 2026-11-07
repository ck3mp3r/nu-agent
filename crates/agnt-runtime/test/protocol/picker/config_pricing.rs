//! The picker's cost column for a model only a configured price prices.

use super::*;

// region:    --- config pricing (rung 4)

#[test]
fn test_picker_build_model_picker_catalog_cost_column_shows_the_config_price() -> Result<()> {
    // -- Setup & Fixtures: the cache lists llama3.3 with no cost and no vendor
    // prices that id, so only the configured price can price it.
    let cache = models_cache(&[provider_spec_with_cost(
        "ollama-cloud",
        "Ollama",
        &[("llama3.3", "Llama 3.3", 128_000, 4_096, None)],
    )]);
    let mut config = plugin_config(&[("ollama", &["llama3.3"])]);
    let Some(provider) = config.providers.get_mut("ollama") else {
        return Err("the fixture must configure ollama".into());
    };
    let Some(model) = provider.models.get_mut("llama3.3") else {
        return Err("the fixture must configure the model".into());
    };
    model.pricing = Some(ModelCost {
        input: 0.5,
        output: 1.5,
        cache_read: None,
        cache_write: None,
    });

    // -- Exec
    let options = build_model_picker_catalog(Some(&cache), &config, "ollama/llama3.3");

    // -- Check
    let option = options.first().ok_or("catalog must hold llama3.3")?;
    assert_eq!(
        option.cost.as_deref(),
        Some("$0.50/$1.50"),
        "the cost column must show the configured price"
    );
    Ok(())
}

#[test]
fn test_picker_build_model_picker_catalog_without_cache_shows_the_config_price() -> Result<()> {
    // -- Setup & Fixtures: no models cache, so only the configured price can
    // price the model.
    let mut config = plugin_config(&[("ollama", &["llama3.3"])]);
    let Some(provider) = config.providers.get_mut("ollama") else {
        return Err("the fixture must configure ollama".into());
    };
    let Some(model) = provider.models.get_mut("llama3.3") else {
        return Err("the fixture must configure the model".into());
    };
    model.pricing = Some(ModelCost {
        input: 0.5,
        output: 1.5,
        cache_read: None,
        cache_write: None,
    });

    // -- Exec
    let options = build_model_picker_catalog(None, &config, "ollama/llama3.3");

    // -- Check
    let option = options.first().ok_or("catalog must hold llama3.3")?;
    assert_eq!(
        option.cost.as_deref(),
        Some("$0.50/$1.50"),
        "the cost column must show the configured price without a cache"
    );
    Ok(())
}

// endregion: --- config pricing (rung 4)
