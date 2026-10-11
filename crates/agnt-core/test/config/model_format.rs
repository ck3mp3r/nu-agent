//! Multi-part model-format tests (`provider/backend/model`).

use std::collections::HashMap;

use crate::config::{AgentsConfig, ModelRoleConfig, PluginConfig, ProviderConfig};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn resolve_model_handles_two_part_format() -> Result<()> {
    // Test that traditional 2-part format still works (backward compatibility)
    let plugin_config = PluginConfig {
        models: {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "openai/gpt-4".to_string(),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers: {
            let mut providers = HashMap::new();
            providers.insert(
                "openai".to_string(),
                ProviderConfig {
                    name: None,
                    api_key: Some("sk-test123".to_string()),
                    base_url: None,
                    provider: None,
                    preamble: None,
                    models: HashMap::new(),
                },
            );
            providers
        },
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
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve 2-part format: {e:?}"))?;

    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4");
    assert_eq!(config.api_key, Some("sk-test123".to_string()));
    Ok(())
}

#[test]
fn resolve_model_validates_empty_parts() -> Result<()> {
    // Test that empty parts in model specification are rejected
    let plugin_config = PluginConfig {
        models: {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "provider/model".to_string(),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers: HashMap::new(),
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

    // Empty provider
    let result = plugin_config.resolve_model(&ModelRoleConfig {
        model: "/model".to_string(),
        ..ModelRoleConfig::default()
    });
    let err = match result {
        Ok(_) => return Err("empty provider part should fail".into()),
        Err(e) => e,
    };
    assert!(err.contains("cannot be empty"));

    // Empty model
    let result = plugin_config.resolve_model(&ModelRoleConfig {
        model: "provider/".to_string(),
        ..ModelRoleConfig::default()
    });
    let err = match result {
        Ok(_) => return Err("empty model part should fail".into()),
        Err(e) => e,
    };
    assert!(err.contains("cannot be empty"));

    // Both empty
    let result = plugin_config.resolve_model(&ModelRoleConfig {
        model: "/".to_string(),
        ..ModelRoleConfig::default()
    });
    assert!(result.is_err());
    Ok(())
}

#[test]
fn resolve_model_uses_split_once_for_multi_part_models() -> Result<()> {
    let plugin_config = PluginConfig {
        models: {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "github-copilot/anthropic/claude-sonnet-4-20250514".to_string(),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers: {
            let mut map = HashMap::new();
            map.insert(
                "github-copilot".to_string(),
                ProviderConfig {
                    name: None,
                    provider: None,
                    api_key: Some("test-key".to_string()),
                    base_url: Some("https://api.githubcopilot.com".to_string()),
                    preamble: None,
                    models: HashMap::new(),
                },
            );
            map
        },
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
        model: "github-copilot/anthropic/claude-sonnet-4-20250514".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("Should resolve github-copilot model: {e:?}"))?;

    // Provider should be "github-copilot"
    assert_eq!(config.provider, "github-copilot");

    // Model should be "anthropic/claude-sonnet-4-20250514" (everything after first /)
    assert_eq!(config.model, "anthropic/claude-sonnet-4-20250514");

    // API key should come from provider config
    assert_eq!(config.api_key, Some("test-key".to_string()));
    Ok(())
}

#[test]
fn resolve_model_works_with_simple_two_part() -> Result<()> {
    let plugin_config = PluginConfig {
        models: {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "openai/gpt-4".to_string(),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers: {
            let mut map = HashMap::new();
            map.insert(
                "openai".to_string(),
                ProviderConfig {
                    name: None,
                    provider: None,
                    api_key: Some("test-key".to_string()),
                    base_url: Some("https://api.githubcopilot.com".to_string()),
                    preamble: None,
                    models: HashMap::new(),
                },
            );
            map
        },
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
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("Should resolve openai model: {e:?}"))?;

    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4");
    Ok(())
}

#[test]
fn integration_github_copilot_with_backend_in_model() -> Result<()> {
    // This simulates the full flow:
    // 1. Config has github-copilot provider
    // 2. User specifies model as "github-copilot/anthropic/claude-sonnet-4-20250514"
    // 3. resolve_model extracts provider="github-copilot", model="anthropic/claude-sonnet-4-20250514"
    // 4. github-copilot provider receives model string and parses backend internally

    let plugin_config = PluginConfig {
        models: {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "github-copilot/anthropic/claude-sonnet-4-20250514".to_string(),
                    ..ModelRoleConfig::default()
                },
            );
            m.insert(
                "light".to_string(),
                ModelRoleConfig {
                    model: "github-copilot/openai/gpt-4o-mini".to_string(),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers: {
            let mut map = HashMap::new();
            map.insert(
                "github-copilot".to_string(),
                ProviderConfig {
                    name: None,
                    provider: None,
                    api_key: Some("test-key".to_string()),
                    base_url: Some("https://api.githubcopilot.com".to_string()),
                    preamble: None,
                    models: HashMap::new(),
                },
            );
            map
        },
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

    // Test default model
    let default_role = ModelRoleConfig {
        model: "github-copilot/anthropic/claude-sonnet-4-20250514".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&default_role)
        .map_err(|e| format!("Should resolve github-copilot anthropic model: {e:?}"))?;

    assert_eq!(config.provider, "github-copilot");
    assert_eq!(config.model, "anthropic/claude-sonnet-4-20250514");
    assert_eq!(config.api_key, Some("test-key".to_string()));
    assert_eq!(
        config.base_url,
        Some("https://api.githubcopilot.com".to_string())
    );

    // Test small model (OpenAI backend)
    let light_role = ModelRoleConfig {
        model: "github-copilot/openai/gpt-4o-mini".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&light_role)
        .map_err(|e| format!("Should resolve github-copilot openai model: {e:?}"))?;

    assert_eq!(config.provider, "github-copilot");
    assert_eq!(config.model, "openai/gpt-4o-mini");
    assert_eq!(config.api_key, Some("test-key".to_string()));
    Ok(())
}
