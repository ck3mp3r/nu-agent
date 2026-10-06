//! `PluginConfig::resolve_model()` core resolution tests.

use std::collections::HashMap;

use super::support::plugin_config_with_cache;
use crate::config::{
    AgentsConfig, ModelConfig, ModelLimits, ModelRoleConfig, PluginConfig, ProviderConfig,
};
use crate::utils::env_map::EnvMap;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_model_role_config_default() {
    let cfg = ModelRoleConfig::default();
    assert_eq!(cfg.model, "");
    assert_eq!(cfg.temperature, None);
    assert_eq!(cfg.max_tokens, None);
    assert_eq!(cfg.max_context_tokens, None);
    assert_eq!(cfg.max_output_tokens, None);
    assert_eq!(cfg.max_tool_turns, None);
    assert_eq!(cfg.max_tool_result_bytes, None);
    assert_eq!(cfg.max_tool_calls_per_subturn, None);
    assert_eq!(cfg.model_context_tokens, None);
    assert_eq!(cfg.context_warning_threshold, None);
    assert_eq!(cfg.additional_params, None);
    assert_eq!(cfg.read_timeout_secs, None);
    assert_eq!(cfg.max_retries, None);
    assert_eq!(cfg.retry_base_delay_ms, None);
}

#[test]
fn test_resolve_model_basic() -> Result<()> {
    // Test resolving a basic model specification
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
                    models: {
                        let mut models = HashMap::new();
                        models.insert(
                            "gpt-4".to_string(),
                            ModelConfig {
                                name: None,
                                temperature: Some(0.7),
                                preamble: None,
                                tool_call: Some(true),
                                limit: Some(ModelLimits {
                                    context: Some(128000),
                                    output: Some(4096),
                                }),
                            },
                        );
                        models
                    },
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
        .map_err(|e| format!("should resolve: {e:?}"))?;

    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4");
    assert_eq!(config.api_key, Some("sk-test123".to_string()));
    assert_eq!(config.temperature, Some(0.7));
    assert_eq!(config.max_context_tokens, Some(128000));
    assert_eq!(config.max_output_tokens, Some(4096));
    Ok(())
}

/// Unset `max_output_tokens` + cache entry → filled with cache `limit.output`.
#[test]
fn test_resolve_model_cache_fills_unset_output_tokens() -> Result<()> {
    // -- Setup & Fixtures
    let plugin_config = plugin_config_with_cache();
    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        ..ModelRoleConfig::default()
    };

    // -- Exec
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    // -- Check
    assert_eq!(config.max_output_tokens, Some(4096));
    Ok(())
}

/// Explicit `max_output_tokens` lower than cache `limit.output` → preserved.
#[test]
fn test_resolve_model_cache_preserves_explicit_lower_output_tokens() -> Result<()> {
    // -- Setup & Fixtures
    let plugin_config = plugin_config_with_cache();
    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        max_output_tokens: Some(1024),
        ..ModelRoleConfig::default()
    };

    // -- Exec
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    // -- Check
    assert_eq!(config.max_output_tokens, Some(1024));
    Ok(())
}

/// Explicit `max_output_tokens` higher than cache `limit.output` → clamped to cache value.
#[test]
fn test_resolve_model_cache_clamps_explicit_higher_output_tokens() -> Result<()> {
    // -- Setup & Fixtures
    let plugin_config = plugin_config_with_cache();
    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        max_output_tokens: Some(65536),
        ..ModelRoleConfig::default()
    };

    // -- Exec
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    // -- Check
    assert_eq!(config.max_output_tokens, Some(4096));
    Ok(())
}

/// No cache entry → `max_output_tokens` unchanged (not set from cache).
#[test]
fn test_resolve_model_no_cache_leaves_output_tokens_unchanged() -> Result<()> {
    // -- Setup & Fixtures
    let mut plugin_config = plugin_config_with_cache();
    plugin_config.models_cache = None;
    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        max_output_tokens: Some(2048),
        ..ModelRoleConfig::default()
    };

    // -- Exec
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    // -- Check
    assert_eq!(config.max_output_tokens, Some(2048));
    Ok(())
}

#[test]
fn test_resolve_model_with_env_fallback() -> Result<()> {
    // Test that resolve_model falls back to env vars when provider doesn't have api_key
    let plugin_config = PluginConfig {
        models: {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "anthropic/claude".to_string(),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers: {
            let mut providers = HashMap::new();
            providers.insert(
                "anthropic".to_string(),
                ProviderConfig {
                    name: None,
                    api_key: None, // No API key in config
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
        model: "anthropic/claude".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude");
    // API key should be None (will be read from env later)
    assert_eq!(config.api_key, None);
    Ok(())
}

#[test]
fn test_resolve_model_invalid_format() -> Result<()> {
    // Test that invalid model format returns error
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

    // No slash separator
    let result = plugin_config.resolve_model(&ModelRoleConfig {
        model: "openaigpt4".to_string(),
        ..ModelRoleConfig::default()
    });
    let err = match result {
        Ok(_) => return Err("missing slash should fail resolution".into()),
        Err(e) => e,
    };
    assert!(err.contains("Expected 'provider/model'"));

    // Empty provider
    let result = plugin_config.resolve_model(&ModelRoleConfig {
        model: "/gpt-4".to_string(),
        ..ModelRoleConfig::default()
    });
    assert!(result.is_err());

    // Empty model
    let result = plugin_config.resolve_model(&ModelRoleConfig {
        model: "openai/".to_string(),
        ..ModelRoleConfig::default()
    });
    assert!(result.is_err());
    Ok(())
}

#[test]
fn test_resolve_model_provider_not_found() -> Result<()> {
    // Test that unknown provider resolves successfully (provider block is optional)
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

    let result = plugin_config.resolve_model(&ModelRoleConfig {
        model: "unknown/model".to_string(),
        ..ModelRoleConfig::default()
    });
    // Provider block is optional — should resolve with env-based config only
    let config = result.map_err(|e| format!("unknown provider should resolve: {e:?}"))?;
    assert_eq!(config.provider, "unknown");
    assert_eq!(config.model, "model");
    Ok(())
}

#[test]
fn test_resolve_model_model_not_in_config() -> Result<()> {
    // An empty env map isolates the test from AGENT_* vars in the surrounding
    // environment (e.g. nix build sets AGENT_TEMPERATURE), which would
    // otherwise be picked up by Config::from_env and break the default
    // expectations.
    let env = EnvMap::new();

    // Test that model not in provider's models map still works (uses defaults)
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
                    models: HashMap::new(), // Empty models map
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
        model: "openai/gpt-3.5-turbo".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model_with(&role_config, &env)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-3.5-turbo");
    assert_eq!(config.api_key, Some("sk-test123".to_string()));
    // No model-specific config, so should use defaults
    assert_eq!(config.temperature, None);
    Ok(())
}

#[test]
fn test_resolve_model_with_provider_field() -> Result<()> {
    // Test resolving with custom provider field (like github-copilot)
    let plugin_config = PluginConfig {
        models: {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "copilot/claude".to_string(),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers: {
            let mut providers = HashMap::new();
            providers.insert(
                "copilot".to_string(),
                ProviderConfig {
                    name: Some("GitHub Copilot".to_string()),
                    api_key: Some("ghcp-token".to_string()),
                    base_url: Some("https://api.githubcopilot.com".to_string()),
                    provider: Some("openai".to_string()), // Use OpenAI API
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
        model: "copilot/claude".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    assert_eq!(config.provider, "copilot");
    assert_eq!(config.model, "claude");
    assert_eq!(config.api_key, Some("ghcp-token".to_string()));
    assert_eq!(
        config.base_url,
        Some("https://api.githubcopilot.com".to_string())
    );
    assert_eq!(config.provider_impl, Some("openai".to_string()));
    Ok(())
}

#[test]
fn test_resolve_model_merges_limits() -> Result<()> {
    // Test that model limits are properly merged into Config
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
                    api_key: None,
                    base_url: None,
                    provider: None,
                    preamble: None,
                    models: {
                        let mut models = HashMap::new();
                        models.insert(
                            "gpt-4".to_string(),
                            ModelConfig {
                                name: Some("GPT-4".to_string()),
                                temperature: None,
                                preamble: None,
                                tool_call: None,
                                limit: Some(ModelLimits {
                                    context: Some(128000),
                                    output: Some(8192),
                                }),
                            },
                        );
                        models
                    },
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
        .map_err(|e| format!("should resolve: {e:?}"))?;

    assert_eq!(config.max_context_tokens, Some(128000));
    assert_eq!(config.max_output_tokens, Some(8192));
    Ok(())
}
