use super::runtime_build;
use nu_agent_core::config::Config;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ============================================================================
// Persona Model Precedence Tests - Fix for persona model override bug
// ============================================================================

#[test]
fn apply_persona_model_overrides_plugin_config() -> Result<()> {
    use std::collections::HashMap;

    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        provider_impl: None,
        ..Config::default()
    };

    // Build a minimal plugin config with the required provider
    let mut models = HashMap::new();
    models.insert(
        "default".to_string(),
        nu_agent_core::config::ModelRoleConfig {
            model: "openai/gpt-4o".to_string(),
            ..Default::default()
        },
    );
    let mut providers = HashMap::new();
    providers.insert(
        "github-copilot".to_string(),
        nu_agent_core::config::ProviderConfig {
            name: None,
            api_key: None,
            base_url: None,
            provider: None,
            preamble: None,
            models: HashMap::new(),
        },
    );
    let plugin_config = nu_agent_core::config::PluginConfig {
        models,
        providers,
        compaction: None,
        agents: Default::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: None,
        permissions: None,
        mcp: None,
        theme: None,
    };

    let applied = runtime_build::apply_persona_model(
        &mut config,
        Some(&plugin_config),
        Some("github-copilot/claude-opus-4.6"),
        false,
    );

    let applied = applied.map_err(|e| format!("{e:?}"))?;
    assert!(applied, "Should apply persona model");
    assert_eq!(config.provider, "github-copilot");
    assert_eq!(config.model, "claude-opus-4.6");
    assert_eq!(config.provider_impl, None);
    Ok(())
}

#[test]
fn apply_persona_model_cli_wins() -> Result<()> {
    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        provider_impl: None,
        ..Config::default()
    };

    let applied = runtime_build::apply_persona_model(
        &mut config,
        None,
        Some("github-copilot/claude-opus-4.6"),
        true, // CLI model was provided
    );

    let applied = applied.map_err(|e| format!("{e:?}"))?;
    assert!(!applied, "Should NOT apply persona model when CLI provided");
    assert_eq!(config.provider, "openai", "Config should be unchanged");
    assert_eq!(config.model, "gpt-4o", "Config should be unchanged");
    Ok(())
}

#[test]
fn apply_persona_model_no_slash_ignored() -> Result<()> {
    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        provider_impl: None,
        ..Config::default()
    };

    let applied =
        runtime_build::apply_persona_model(&mut config, None, Some("just-a-model"), false);

    assert!(
        applied.is_err(),
        "Should error when no plugin config and no slash"
    );
    Ok(())
}

#[test]
fn apply_persona_model_none_preserves_config() -> Result<()> {
    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        provider_impl: None,
        ..Config::default()
    };

    let applied = runtime_build::apply_persona_model(&mut config, None, None, false);

    let applied = applied.map_err(|e| format!("{e:?}"))?;
    assert!(!applied, "Should NOT apply when persona model is None");
    assert_eq!(config.provider, "openai", "Config should be unchanged");
    assert_eq!(config.model, "gpt-4o", "Config should be unchanged");
    Ok(())
}

#[test]
fn apply_persona_model_clears_provider_impl() -> Result<()> {
    use std::collections::HashMap;

    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        provider_impl: Some("openai".to_string()),
        ..Config::default()
    };

    // Build a minimal plugin config with the required provider
    let mut models = HashMap::new();
    models.insert(
        "default".to_string(),
        nu_agent_core::config::ModelRoleConfig {
            model: "openai/gpt-4o".to_string(),
            ..Default::default()
        },
    );
    let mut providers = HashMap::new();
    providers.insert(
        "anthropic".to_string(),
        nu_agent_core::config::ProviderConfig {
            name: None,
            api_key: None,
            base_url: None,
            provider: None,
            preamble: None,
            models: HashMap::new(),
        },
    );
    let plugin_config = nu_agent_core::config::PluginConfig {
        models,
        providers,
        compaction: None,
        agents: Default::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: None,
        permissions: None,
        mcp: None,
        theme: None,
    };

    let applied = runtime_build::apply_persona_model(
        &mut config,
        Some(&plugin_config),
        Some("anthropic/claude-sonnet-4-20250514"),
        false,
    );

    let applied = applied.map_err(|e| format!("{e:?}"))?;
    assert!(applied, "Should apply persona model");
    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude-sonnet-4-20250514");
    assert_eq!(
        config.provider_impl, None,
        "provider_impl should be cleared"
    );
    Ok(())
}
