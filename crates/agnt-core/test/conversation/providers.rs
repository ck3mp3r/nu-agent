use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn resolve_provider_type_uses_explicit_field() {
    assert_eq!(
        super::resolve_provider_type("ollama-remote", Some("ollama")),
        "ollama"
    );
}

#[test]
fn resolve_provider_type_falls_back_to_key() {
    assert_eq!(super::resolve_provider_type("ollama", None), "ollama");
}

#[test]
fn resolve_provider_type_custom_key_with_known_impl() {
    assert_eq!(
        super::resolve_provider_type("my-openai", Some("openai")),
        "openai"
    );
}

// ========================================================================
// Phase 1b: CachedProviderClient, resolve_provider_type, ClientCacheKey
// ========================================================================

#[test]
fn client_cache_key_type_is_four_tuple() {
    let key: ClientCacheKey = ("copilot".to_string(), None, None, None);
    assert_eq!(key.0, "copilot");
    assert_eq!(key.1, None);
    assert_eq!(key.2, None);
    assert_eq!(key.3, None);
}

#[test]
fn resolve_provider_type_uses_field_when_set() {
    assert_eq!(
        super::resolve_provider_type("my-provider", Some("copilot")),
        "copilot"
    );
}

#[test]
fn resolve_provider_type_falls_back_to_key_when_field_none() {
    assert_eq!(
        super::resolve_provider_type("github-copilot", None),
        "github-copilot"
    );
}

// ========================================================================
// read_timeout_secs pass-through tests
// ========================================================================

#[test]
fn plugin_config_read_timeout_secs_propagates_to_resolved_config() -> Result<()> {
    use std::collections::HashMap;

    use crate::config::{AgentsConfig, ModelRoleConfig, PluginConfig, ProviderConfig};

    let mut providers = HashMap::new();
    providers.insert(
        "openai".to_string(),
        ProviderConfig {
            name: None,
            api_key: Some("sk-test".to_string()),
            base_url: None,
            provider: None,
            preamble: None,
            models: HashMap::new(),
        },
    );

    let plugin_config = PluginConfig {
        models: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "openai/gpt-4".to_string(),
                    read_timeout_secs: Some(60),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers,
        compaction: None,
        agents: AgentsConfig::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: None,
        permissions: None,
        mcp: None,
        theme: None,
    };

    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        read_timeout_secs: Some(60),
        ..ModelRoleConfig::default()
    };
    let resolved = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    assert_eq!(
        resolved.read_timeout_secs,
        Some(60),
        "read_timeout_secs should be 60 after resolve"
    );
    Ok(())
}

#[test]
fn plugin_config_without_read_timeout_secs_resolves_to_none() -> Result<()> {
    use std::collections::HashMap;

    use crate::config::{AgentsConfig, ModelRoleConfig, PluginConfig, ProviderConfig};

    let mut providers = HashMap::new();
    providers.insert(
        "openai".to_string(),
        ProviderConfig {
            name: None,
            api_key: Some("sk-test".to_string()),
            base_url: None,
            provider: None,
            preamble: None,
            models: HashMap::new(),
        },
    );

    let plugin_config = PluginConfig {
        models: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "openai/gpt-4".to_string(),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers,
        compaction: None,
        agents: AgentsConfig::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: None,
        permissions: None,
        mcp: None,
        theme: None,
    };

    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        ..ModelRoleConfig::default()
    };
    let resolved = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    assert_eq!(
        resolved.read_timeout_secs, None,
        "read_timeout_secs should be None when not configured"
    );
    Ok(())
}
