//! Unit tests for the model and agent config option builders.
//!
//! Every test drives `build_model_config_options` or `build_agent_config_options`
//! directly, so nothing touches the network.

use agent_client_protocol::schema::v1::{
    SessionConfigKind, SessionConfigOptionCategory, SessionConfigSelectOptions,
};
use agnt_core::config::ModelsCache;

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn config(provider: &str, model: &str) -> Config {
    Config {
        provider: provider.to_string(),
        model: model.to_string(),
        ..Config::default()
    }
}

/// A config whose provider block declares the cache section it serves.
fn config_with_catalog(provider: &str, model: &str, catalog: &str) -> Config {
    Config {
        catalog: Some(catalog.to_string()),
        ..config(provider, model)
    }
}

/// A models cache holding `models` under `provider`.
fn models_cache(provider: &str, models: &[(&str, &str)]) -> Result<ModelsCache> {
    let models: serde_json::Map<String, serde_json::Value> = models
        .iter()
        .map(|(id, name)| {
            (
                id.to_string(),
                serde_json::json!({
                    "id": id,
                    "name": name,
                    "tool_call": true,
                    "limit": { "context": 8192, "output": 4096 },
                }),
            )
        })
        .collect();
    let json = serde_json::json!({
        (provider): {
            "id": provider,
            "name": provider,
            "models": models,
        }
    });
    Ok(serde_json::from_value(json)?)
}

/// The selectable values of the first option.
fn select_values(options: &[SessionConfigOption]) -> Vec<String> {
    let Some(option) = options.first() else {
        return Vec::new();
    };
    let SessionConfigKind::Select(select) = &option.kind else {
        return Vec::new();
    };
    match &select.options {
        SessionConfigSelectOptions::Ungrouped(values) => {
            values.iter().map(|value| value.value.to_string()).collect()
        }
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .map(|value| value.value.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// The current value of the first option.
fn current_value(options: &[SessionConfigOption]) -> Option<String> {
    let option = options.first()?;
    let SessionConfigKind::Select(select) = &option.kind else {
        return None;
    };
    Some(select.current_value.to_string())
}

// ---------------------------------------------------------------------------
// build_model_config_options
// ---------------------------------------------------------------------------

#[test]
fn test_build_model_config_options_with_cache_lists_provider_models() -> Result<()> {
    // -- Setup & Fixtures
    let config = config_with_catalog("ollama", "llama3", "ollama-cloud");
    // The cache files ollama's models under `ollama-cloud`, as models.dev does.
    let cache = models_cache(
        "ollama-cloud",
        &[("llama3", "Llama 3"), ("llama4", "Llama 4")],
    )?;

    // -- Exec
    let options = build_model_config_options(&config, Some(&cache));

    // -- Check
    assert_eq!(options.len(), 1, "exactly one model option is advertised");
    assert_eq!(current_value(&options).as_deref(), Some("ollama/llama3"));
    let values = select_values(&options);
    assert!(
        values.contains(&"ollama/llama3".to_string()),
        "the configured model must be selectable; got: {values:?}"
    );
    assert!(
        values.contains(&"ollama/llama4".to_string()),
        "every cached model of the provider must be selectable; got: {values:?}"
    );
    Ok(())
}

#[test]
fn test_build_model_config_options_sets_model_category() -> Result<()> {
    // -- Setup & Fixtures
    let config = config("ollama", "llama3");

    // -- Exec
    let options = build_model_config_options(&config, None);

    // -- Check
    let option = options.first().ok_or("one option must be advertised")?;
    assert_eq!(option.id.to_string(), MODEL_CONFIG_ID);
    assert_eq!(option.category, Some(SessionConfigOptionCategory::Model));
    Ok(())
}

#[test]
fn test_build_agent_config_options_sets_mode_category() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let agents_config = AgentsConfig::default();

    // -- Exec
    let options = build_agent_config_options(
        &agents_config,
        temp.path(),
        &temp.path().join("agents"),
        None,
    );

    // -- Check
    let option = options.first().ok_or("one option must be advertised")?;
    assert_eq!(option.id.to_string(), AGENT_CONFIG_ID);
    assert_eq!(option.category, Some(SessionConfigOptionCategory::Mode));
    Ok(())
}

#[test]
fn test_build_model_config_options_without_cache_lists_only_current() -> Result<()> {
    // -- Setup & Fixtures
    let config = config("ollama", "llama3");

    // -- Exec
    let options = build_model_config_options(&config, None);

    // -- Check
    assert_eq!(current_value(&options).as_deref(), Some("ollama/llama3"));
    assert_eq!(select_values(&options), vec!["ollama/llama3".to_string()]);
    Ok(())
}

#[test]
fn test_build_model_config_options_cache_without_provider_lists_only_current() -> Result<()> {
    // -- Setup & Fixtures
    let config = config("ollama", "llama3");
    let cache = models_cache("openai", &[("gpt-4o", "GPT-4o")])?;

    // -- Exec
    let options = build_model_config_options(&config, Some(&cache));

    // -- Check
    assert_eq!(select_values(&options), vec!["ollama/llama3".to_string()]);
    Ok(())
}

#[test]
fn test_build_model_config_options_cache_missing_current_keeps_current_selectable() -> Result<()> {
    // -- Setup & Fixtures
    let config = config("ollama", "custom-model");
    let cache = models_cache("ollama-cloud", &[("llama3", "Llama 3")])?;

    // -- Exec
    let options = build_model_config_options(&config, Some(&cache));

    // -- Check
    let values = select_values(&options);
    assert!(
        values.contains(&"ollama/custom-model".to_string()),
        "the configured model must stay selectable even when the cache omits it; got: {values:?}"
    );
    Ok(())
}
