use std::collections::HashMap;

use tempfile::TempDir;

use super::{
    ConfigLoadError, load_and_resolve_config, resolve_preamble_for_model, resolve_role_config,
};
use crate::config::{ModelConfig, ModelRoleConfig, PluginConfig, ProviderConfig};
use crate::utils::env_map::EnvMap;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_loader_load_and_resolve_config_missing_file_returns_not_found() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let missing = dir.path().join("does-not-exist.toml");

    // -- Exec
    let result = load_and_resolve_config(&missing, None);

    // -- Check
    let err = result.err().ok_or("missing config must fail")?;
    assert!(
        matches!(err, ConfigLoadError::NotFound(_)),
        "missing config must produce NotFound, got: {err:?}"
    );
    Ok(())
}

#[test]
fn test_loader_resolve_role_config_no_default_returns_no_default_model() {
    // -- Setup & Fixtures
    let plugin_config = PluginConfig::default();

    // -- Exec
    let result = resolve_role_config(&plugin_config, None, &EnvMap::new());

    // -- Check
    assert!(
        matches!(result, Err(ConfigLoadError::NoDefaultModel)),
        "an empty models map must produce NoDefaultModel, got: {result:?}"
    );
}

#[test]
fn test_loader_resolve_role_config_unknown_label_returns_unknown_role() {
    // -- Setup & Fixtures
    let plugin_config = plugin_config_with_roles(&[("default", "ollama/llama3")]);

    // -- Exec
    let result = resolve_role_config(&plugin_config, Some("heavy"), &EnvMap::new());

    // -- Check
    let role = match result {
        Err(ConfigLoadError::UnknownRole(role)) => role,
        other => format!("expected UnknownRole, got: {other:?}"),
    };
    assert_eq!(role, "heavy");
}

#[test]
fn test_loader_resolve_role_config_literal_override_ignores_models_map() -> Result<()> {
    // -- Setup & Fixtures
    // No `default` role exists; a literal provider/model override must still work.
    let plugin_config = plugin_config_with_roles(&[]);

    // -- Exec
    let config = resolve_role_config(&plugin_config, Some("ollama/llama3"), &EnvMap::new())
        .map_err(|e| format!("should resolve literal override: {e}"))?;

    // -- Check
    assert_eq!(config.provider, "ollama");
    assert_eq!(config.model, "llama3");
    Ok(())
}

#[test]
fn test_loader_resolve_preamble_for_model_returns_provider_preamble() {
    // -- Setup & Fixtures
    let pc = plugin_config_with_preamble(Some("You are OpenAI GPT-4o."), None);

    // -- Exec
    let preamble = resolve_preamble_for_model(&pc, "openai", "gpt-4o");

    // -- Check
    assert_eq!(preamble, Some("You are OpenAI GPT-4o.".to_string()));
}

#[test]
fn test_loader_resolve_preamble_for_model_returns_model_preamble_over_provider() {
    // -- Setup & Fixtures
    let pc =
        plugin_config_with_preamble(Some("You are Anthropic."), Some("You are Claude Sonnet."));

    // -- Exec
    let preamble = resolve_preamble_for_model(&pc, "anthropic", "claude-sonnet-4-6");

    // -- Check
    assert_eq!(preamble, Some("You are Claude Sonnet.".to_string()));
}

#[test]
fn test_loader_resolve_preamble_for_model_unknown_provider_returns_none() {
    // -- Setup & Fixtures
    let pc = plugin_config_with_preamble(Some("You are Anthropic."), None);

    // -- Exec
    let preamble = resolve_preamble_for_model(&pc, "unknown", "model");

    // -- Check
    assert!(
        preamble.is_none(),
        "an unknown provider has no config to resolve a preamble from"
    );
}

#[test]
fn test_loader_resolve_preamble_for_model_falls_back_to_builtin_defaults() {
    // -- Setup & Fixtures
    // Known provider, but no user preamble configured.
    let pc = plugin_config_with_preamble(None, None);

    // -- Exec
    let preamble = resolve_preamble_for_model(&pc, "openai", "gpt-4o");

    // -- Check
    assert!(
        preamble.is_some_and(|p| !p.is_empty()),
        "a known provider must fall back to the builtin preamble"
    );
}

// endregion: --- Tests

// region:    --- Test Support

fn plugin_config_with_roles(roles: &[(&str, &str)]) -> PluginConfig {
    let mut models = HashMap::new();
    for (label, spec) in roles {
        models.insert(
            label.to_string(),
            ModelRoleConfig {
                model: spec.to_string(),
                ..ModelRoleConfig::default()
            },
        );
    }
    let mut providers = HashMap::new();
    providers.insert(
        "ollama".to_string(),
        ProviderConfig {
            base_url: Some("http://localhost:11434".to_string()),
            ..ProviderConfig::default()
        },
    );
    PluginConfig {
        models,
        providers,
        ..PluginConfig::default()
    }
}

fn plugin_config_with_preamble(
    provider_preamble: Option<&str>,
    model_preamble: Option<&str>,
) -> PluginConfig {
    let mut models = HashMap::new();
    for model in ["gpt-4o", "claude-sonnet-4-6"] {
        models.insert(
            model.to_string(),
            ModelConfig {
                preamble: model_preamble.map(|s| s.to_string()),
                ..ModelConfig::default()
            },
        );
    }
    let mut providers = HashMap::new();
    for provider in ["openai", "anthropic"] {
        providers.insert(
            provider.to_string(),
            ProviderConfig {
                preamble: provider_preamble.map(|s| s.to_string()),
                models: models.clone(),
                ..ProviderConfig::default()
            },
        );
    }
    PluginConfig {
        providers,
        ..PluginConfig::default()
    }
}

// endregion: --- Test Support
