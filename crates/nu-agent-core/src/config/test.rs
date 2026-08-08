use super::*;
use crate::compaction::CompactionStrategy;
use crate::session::StoreType;
use serial_test::serial;
use std::env;

// Helper to set env vars for tests
fn with_env_vars<F>(vars: Vec<(&str, &str)>, test: F)
where
    F: FnOnce(),
{
    // Set vars
    for (key, val) in &vars {
        unsafe {
            env::set_var(key, val);
        }
    }

    // Run test
    test();

    // Cleanup
    for (key, _) in &vars {
        unsafe {
            env::remove_var(key);
        }
    }
}

#[test]
#[serial]
fn test_from_env_with_provider_api_key() {
    // Test reading provider-specific API key from environment
    with_env_vars(vec![("OPENAI_API_KEY", "sk-test123")], || {
        let config = Config::from_env("openai", "gpt-4");

        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4");
        assert_eq!(config.api_key, Some("sk-test123".to_string()));
        assert!(config.max_tool_turns.is_none()); // Default is now None
    });
}

#[test]
#[serial]
fn test_from_env_missing_api_key() {
    // Test that missing API key results in None (graceful handling)
    let config = Config::from_env("nonexistent", "model-1");

    assert_eq!(config.provider, "nonexistent");
    assert_eq!(config.model, "model-1");
    assert!(config.api_key.is_none());
}

#[test]
#[serial]
fn test_from_env_with_agent_overrides() {
    // Test AGENT_* environment variable overrides
    with_env_vars(
        vec![
            ("ANTHROPIC_API_KEY", "sk-ant-test"),
            ("AGENT_TEMPERATURE", "0.8"),
            ("AGENT_MAX_TOKENS", "2000"),
            ("AGENT_MAX_CONTEXT_TOKENS", "8192"),
            ("AGENT_MAX_OUTPUT_TOKENS", "4096"),
            ("AGENT_MAX_TOOL_TURNS", "15"),
            ("AGENT_BASE_URL", "https://custom.api.com"),
            ("AGENT_MAX_TOOL_RESULT_BYTES", "15000"),
        ],
        || {
            let config = Config::from_env("anthropic", "claude-3-opus");

            assert_eq!(config.provider, "anthropic");
            assert_eq!(config.model, "claude-3-opus");
            assert_eq!(config.api_key, Some("sk-ant-test".to_string()));
            assert_eq!(config.base_url, Some("https://custom.api.com".to_string()));
            assert_eq!(config.temperature, Some(0.8));
            assert_eq!(config.max_tokens, Some(2000));
            assert_eq!(config.max_context_tokens, Some(8192));
            assert_eq!(config.max_output_tokens, Some(4096));
            assert_eq!(config.max_tool_turns, Some(15));
            assert_eq!(config.max_tool_result_bytes, Some(15_000));
        },
    );
}

#[test]
#[serial]
fn test_from_env_partial_overrides() {
    // Test with only some AGENT_* vars set
    with_env_vars(
        vec![
            ("OPENAI_API_KEY", "sk-partial"),
            ("AGENT_TEMPERATURE", "0.5"),
        ],
        || {
            let config = Config::from_env("openai", "gpt-3.5-turbo");

            assert_eq!(config.temperature, Some(0.5));
            assert!(config.max_tokens.is_none());
            assert!(config.base_url.is_none());
            assert!(config.max_tool_turns.is_none()); // Default is None, not overridden
        },
    );
}

#[test]
#[serial]
fn test_from_env_invalid_numeric_values() {
    // Test that invalid numeric values are ignored (None)
    with_env_vars(
        vec![
            ("AGENT_TEMPERATURE", "not-a-number"),
            ("AGENT_MAX_TOKENS", "invalid"),
            ("AGENT_MAX_TOOL_TURNS", "-5"),
        ],
        || {
            let config = Config::from_env("openai", "gpt-4");

            // Invalid values should be None, not panic
            assert!(config.temperature.is_none());
            assert!(config.max_tokens.is_none());
            assert!(config.max_tool_turns.is_none()); // Default is None
        },
    );
}

#[test]
#[serial]
fn test_from_env_case_sensitivity() {
    // Test that provider name is uppercased for env var lookup
    with_env_vars(vec![("OPENAI_API_KEY", "sk-case-test")], || {
        // Should work with lowercase provider name
        let config = Config::from_env("openai", "gpt-4");
        assert_eq!(config.api_key, Some("sk-case-test".to_string()));

        // Should also work with mixed case
        let config2 = Config::from_env("OpenAI", "gpt-4");
        assert_eq!(config2.api_key, Some("sk-case-test".to_string()));
    });
}

#[test]
#[serial]
fn test_from_env_max_retries_and_delay() {
    with_env_vars(
        vec![
            ("AGENT_MAX_RETRIES", "7"),
            ("AGENT_RETRY_BASE_DELAY_MS", "500"),
        ],
        || {
            let config = Config::from_env("openai", "gpt-4");
            assert_eq!(config.max_retries, Some(7u8));
            assert_eq!(config.retry_base_delay_ms, Some(500u64));
        },
    );
}

#[test]
#[serial]
fn test_from_env_read_timeout_secs() {
    with_env_vars(vec![("AGENT_READ_TIMEOUT_SECS", "60")], || {
        let config = Config::from_env("openai", "gpt-4");
        assert_eq!(config.read_timeout_secs, Some(60u64));
    });
}

#[test]
fn test_validate_valid_config() {
    // Test that a valid config passes validation
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: Some("test-key".to_string()),
        base_url: Some("https://api.com".to_string()),
        temperature: Some(0.7),
        max_tokens: Some(1000),
        max_context_tokens: Some(4096),
        max_output_tokens: Some(2048),
        max_tool_turns: Some(20),
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_minimal_config() {
    // Test that minimal config with only required fields passes
    let config = Config {
        a2a_port: None,
        provider: "anthropic".to_string(),
        provider_impl: None,
        model: "claude-3-opus".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_empty_provider() {
    // Test that empty provider fails validation
    let config = Config {
        a2a_port: None,
        provider: String::new(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    let result = config.validate();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("provider"));
}

#[test]
fn test_validate_empty_model() {
    // Test that empty model fails validation
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        provider_impl: None,
        model: String::new(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    let result = config.validate();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("model"));
}

#[test]
fn test_validate_max_output_exceeds_context() {
    // Test that max_output_tokens > max_context_tokens fails
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: Some(2000),
        max_output_tokens: Some(3000), // Exceeds context
        max_tool_turns: Some(20),
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    let result = config.validate();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("max_output_tokens"));
    assert!(err.contains("max_context_tokens"));
}

#[test]
fn test_validate_max_output_equals_context() {
    // Test that max_output_tokens == max_context_tokens is valid
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: Some(4000),
        max_output_tokens: Some(4000), // Equal is OK
        max_tool_turns: Some(20),
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_zero_max_tool_turns() {
    // Test that max_tool_turns = 0 fails
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(0), // Invalid
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    let result = config.validate();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("max_tool_turns"));
}

#[test]
fn test_validate_only_context_tokens_set() {
    // Test that only max_context_tokens set (no max_output_tokens) is valid
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: Some(4096),
        max_output_tokens: None,
        max_tool_turns: Some(20),
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_only_output_tokens_set() {
    // Test that only max_output_tokens set (no max_context_tokens) is valid
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: Some(2048),
        max_tool_turns: Some(20),
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_context_warning_threshold_zero_is_err() {
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        context_warning_threshold: Some(0.0),
        ..Config::default()
    };
    let result = config.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("context_warning_threshold"));
}

#[test]
fn test_validate_context_warning_threshold_above_one_is_err() {
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        context_warning_threshold: Some(1.1),
        ..Config::default()
    };
    let result = config.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("context_warning_threshold"));
}

#[test]
fn test_validate_context_warning_threshold_one_is_ok() {
    // Boundary: 1.0 is valid
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        context_warning_threshold: Some(1.0),
        ..Config::default()
    };
    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_context_warning_threshold_typical_is_ok() {
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        context_warning_threshold: Some(0.6),
        ..Config::default()
    };
    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_model_context_tokens_zero_is_err() {
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        model_context_tokens: Some(0),
        ..Config::default()
    };
    let result = config.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("model_context_tokens"));
}

#[test]
fn test_validate_model_context_tokens_one_is_ok() {
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        model_context_tokens: Some(1),
        ..Config::default()
    };
    assert!(config.validate().is_ok());
}

// ============================================================================
// ModelRoleConfig Tests
// ============================================================================

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

// ============================================================================
// PluginConfig::resolve_model() Tests
// ============================================================================

#[test]
fn test_resolve_model_basic() {
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
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    };

    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .expect("should resolve");

    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4");
    assert_eq!(config.api_key, Some("sk-test123".to_string()));
    assert_eq!(config.temperature, Some(0.7));
    assert_eq!(config.max_context_tokens, Some(128000));
    assert_eq!(config.max_output_tokens, Some(4096));
}

#[test]
fn test_resolve_model_with_env_fallback() {
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
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    };

    let role_config = ModelRoleConfig {
        model: "anthropic/claude".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .expect("should resolve");

    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude");
    // API key should be None (will be read from env later)
    assert_eq!(config.api_key, None);
}

#[test]
fn test_resolve_model_invalid_format() {
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
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    };

    // No slash separator
    let result = plugin_config.resolve_model(&ModelRoleConfig {
        model: "openaigpt4".to_string(),
        ..ModelRoleConfig::default()
    });
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Expected 'provider/model'"));

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
}

#[test]
fn test_resolve_model_provider_not_found() {
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
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    };

    let result = plugin_config.resolve_model(&ModelRoleConfig {
        model: "unknown/model".to_string(),
        ..ModelRoleConfig::default()
    });
    // Provider block is optional — should resolve with env-based config only
    assert!(result.is_ok());
    let config = result.unwrap();
    assert_eq!(config.provider, "unknown");
    assert_eq!(config.model, "model");
}

#[test]
fn test_resolve_model_model_not_in_config() {
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
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    };

    let role_config = ModelRoleConfig {
        model: "openai/gpt-3.5-turbo".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .expect("should resolve");

    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-3.5-turbo");
    assert_eq!(config.api_key, Some("sk-test123".to_string()));
    // No model-specific config, so should use defaults
    assert_eq!(config.temperature, None);
}

#[test]
fn test_resolve_model_with_provider_field() {
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
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    };

    let role_config = ModelRoleConfig {
        model: "copilot/claude".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .expect("should resolve");

    assert_eq!(config.provider, "copilot");
    assert_eq!(config.model, "claude");
    assert_eq!(config.api_key, Some("ghcp-token".to_string()));
    assert_eq!(
        config.base_url,
        Some("https://api.githubcopilot.com".to_string())
    );
    assert_eq!(config.provider_impl, Some("openai".to_string()));
}

#[test]
fn test_resolve_model_merges_limits() {
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
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    };

    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .expect("should resolve");

    assert_eq!(config.max_context_tokens, Some(128000));
    assert_eq!(config.max_output_tokens, Some(8192));
}

#[test]
fn test_plugin_config_resolve_model_role_level_overrides() {
    let make_config = |model_temperature: Option<f64>| -> PluginConfig {
        let mut models = HashMap::new();
        models.insert(
            "gpt-4".to_string(),
            ModelConfig {
                name: None,
                temperature: model_temperature,
                preamble: None,
                tool_call: None,
                limit: None,
            },
        );
        let mut providers = HashMap::new();
        providers.insert(
            "openai".to_string(),
            ProviderConfig {
                name: None,
                api_key: None,
                base_url: None,
                provider: None,
                preamble: None,
                models,
            },
        );
        PluginConfig {
            models: {
                let mut m = HashMap::new();
                m.insert(
                    "default".to_string(),
                    ModelRoleConfig {
                        model: "openai/gpt-4".to_string(),
                        temperature: Some(0.5),
                        max_tokens: Some(2048),
                        max_context_tokens: Some(32000),
                        max_output_tokens: Some(1024),
                        max_tool_turns: Some(5),
                        max_tool_result_bytes: Some(10000),
                        model_context_tokens: Some(128000),
                        context_warning_threshold: Some(0.8f32),
                        max_retries: Some(5),
                        retry_base_delay_ms: Some(2000),
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
            secret_store: None,
            models_cache: None,
            permissions: None,
            mcp: None,
        }
    };

    let default_role = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        temperature: Some(0.5),
        max_tokens: Some(2048),
        max_context_tokens: Some(32000),
        max_output_tokens: Some(1024),
        max_tool_turns: Some(5),
        max_tool_result_bytes: Some(10000),
        model_context_tokens: Some(128000),
        context_warning_threshold: Some(0.8f32),
        max_retries: Some(5),
        retry_base_delay_ms: Some(2000),
        ..ModelRoleConfig::default()
    };

    // Case 1: no model-level temperature — role-level 0.5 must survive
    let cfg = make_config(None)
        .resolve_model(&default_role)
        .expect("resolve");
    assert_eq!(cfg.temperature, Some(0.5));
    assert_eq!(cfg.max_tokens, Some(2048));
    assert_eq!(cfg.max_context_tokens, Some(32000));
    assert_eq!(cfg.max_output_tokens, Some(1024));
    assert_eq!(cfg.max_tool_turns, Some(5));
    assert_eq!(cfg.max_tool_result_bytes, Some(10000));
    assert_eq!(cfg.model_context_tokens, Some(128000));
    assert_eq!(cfg.context_warning_threshold, Some(0.8f32));
    assert_eq!(cfg.max_retries, Some(5));
    assert_eq!(cfg.retry_base_delay_ms, Some(2000));

    // Case 2: role-level temperature 0.5 must beat model-level 0.9
    // (role config is highest priority within resolve_model)
    let cfg = make_config(Some(0.9))
        .resolve_model(&default_role)
        .expect("resolve");
    assert_eq!(cfg.temperature, Some(0.5));
}
// ============================================================================
// 3-Part Format Tests (github-copilot/backend/model)
// ============================================================================

#[test]
fn resolve_model_handles_two_part_format() {
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
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    };

    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .expect("should resolve 2-part format");

    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4");
    assert_eq!(config.api_key, Some("sk-test123".to_string()));
}

#[test]
fn resolve_model_validates_empty_parts() {
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
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    };

    // Empty provider
    let result = plugin_config.resolve_model(&ModelRoleConfig {
        model: "/model".to_string(),
        ..ModelRoleConfig::default()
    });
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("cannot be empty"));

    // Empty model
    let result = plugin_config.resolve_model(&ModelRoleConfig {
        model: "provider/".to_string(),
        ..ModelRoleConfig::default()
    });
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("cannot be empty"));

    // Both empty
    let result = plugin_config.resolve_model(&ModelRoleConfig {
        model: "/".to_string(),
        ..ModelRoleConfig::default()
    });
    assert!(result.is_err());
}

// ============================================================================
// split_once() Tests - Provider-Agnostic Parsing
// ============================================================================

#[test]
fn resolve_model_uses_split_once_for_multi_part_models() {
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
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    };

    let role_config = ModelRoleConfig {
        model: "github-copilot/anthropic/claude-sonnet-4-20250514".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .expect("Should resolve github-copilot model");

    // Provider should be "github-copilot"
    assert_eq!(config.provider, "github-copilot");

    // Model should be "anthropic/claude-sonnet-4-20250514" (everything after first /)
    assert_eq!(config.model, "anthropic/claude-sonnet-4-20250514");

    // API key should come from provider config
    assert_eq!(config.api_key, Some("test-key".to_string()));
}

#[test]
fn resolve_model_works_with_simple_two_part() {
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
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    };

    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .expect("Should resolve openai model");

    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4");
}

#[test]
fn integration_github_copilot_with_backend_in_model() {
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
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    };

    // Test default model
    let default_role = ModelRoleConfig {
        model: "github-copilot/anthropic/claude-sonnet-4-20250514".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&default_role)
        .expect("Should resolve github-copilot anthropic model");

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
        .expect("Should resolve github-copilot openai model");

    assert_eq!(config.provider, "github-copilot");
    assert_eq!(config.model, "openai/gpt-4o-mini");
    assert_eq!(config.api_key, Some("test-key".to_string()));
}

// RED TEST: max_tool_turns defaults to None (not Some(20))
#[test]
fn test_from_env_max_tool_turns_defaults_to_none() {
    // Default should be None (no default - runtime decides based on mode)
    let config = Config::from_env("openai", "gpt-4");
    assert_eq!(config.max_tool_turns, None);
}

// RED TEST: max_tool_turns None is valid in validation
#[test]
fn test_validate_none_max_tool_turns_is_valid() {
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None, // Should be valid
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    assert!(config.validate().is_ok());
}

// RED TEST: max_tool_turns Some(0) is still invalid
#[test]
fn test_validate_zero_max_tool_turns_still_invalid() {
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(0), // Still invalid
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    let result = config.validate();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("max_tool_turns"));
}

// ============================================================================
// Copilot API Key Fallback Tests
// ============================================================================

#[test]
#[serial]
fn test_from_env_copilot_with_github_copilot_api_key() {
    // Test copilot provider with GITHUB_COPILOT_API_KEY
    // For copilot providers, api_key should be None (rig handles env vars via from_env())
    with_env_vars(vec![("GITHUB_COPILOT_API_KEY", "copilot_key")], || {
        let config = Config::from_env("copilot", "claude");

        assert_eq!(config.provider, "copilot");
        assert_eq!(config.model, "claude");
        assert_eq!(config.api_key, None);
    });
}

#[test]
#[serial]
fn test_from_env_copilot_fallback_to_github_token() {
    // Test copilot provider falls back to GITHUB_TOKEN if GITHUB_COPILOT_API_KEY not set
    // For copilot providers, api_key should be None (rig handles env vars via from_env())
    with_env_vars(vec![("GITHUB_TOKEN", "github_token")], || {
        let config = Config::from_env("copilot", "claude");

        assert_eq!(config.provider, "copilot");
        assert_eq!(config.model, "claude");
        assert_eq!(config.api_key, None);
    });
}

#[test]
#[serial]
fn test_from_env_copilot_precedence_github_copilot_api_key_over_github_token() {
    // Test that GITHUB_COPILOT_API_KEY takes precedence over GITHUB_TOKEN
    // For copilot providers, api_key should be None (rig handles env vars via from_env())
    with_env_vars(
        vec![
            ("GITHUB_COPILOT_API_KEY", "copilot_key"),
            ("GITHUB_TOKEN", "github_token"),
        ],
        || {
            let config = Config::from_env("copilot", "claude");

            assert_eq!(config.provider, "copilot");
            assert_eq!(config.model, "claude");
            assert_eq!(config.api_key, None);
        },
    );
}

#[test]
#[serial]
fn test_from_env_github_copilot_with_github_copilot_api_key() {
    // Test "github-copilot" provider variant
    // For copilot providers, api_key should be None (rig handles env vars via from_env())
    with_env_vars(vec![("GITHUB_COPILOT_API_KEY", "copilot_key")], || {
        let config = Config::from_env("github-copilot", "claude");

        assert_eq!(config.provider, "github-copilot");
        assert_eq!(config.model, "claude");
        assert_eq!(config.api_key, None);
    });
}

#[test]
#[serial]
fn test_from_env_copilot_missing_all_keys() {
    // Test copilot provider without any API keys
    unsafe {
        std::env::remove_var("GITHUB_COPILOT_API_KEY");
        std::env::remove_var("GITHUB_TOKEN");
    }

    let config = Config::from_env("copilot", "claude");

    assert_eq!(config.provider, "copilot");
    assert_eq!(config.model, "claude");
    assert!(config.api_key.is_none());
}

#[test]
#[serial]
fn test_from_env_copilot_case_insensitive() {
    // Test that copilot provider name is case-insensitive
    // For copilot providers, api_key should be None (rig handles env vars via from_env())
    with_env_vars(vec![("GITHUB_COPILOT_API_KEY", "copilot_key")], || {
        // Lowercase
        let config1 = Config::from_env("copilot", "claude");
        assert_eq!(config1.api_key, None);

        // Mixed case
        let config2 = Config::from_env("Copilot", "claude");
        assert_eq!(config2.api_key, None);

        // github-copilot variant
        let config3 = Config::from_env("github-copilot", "claude");
        assert_eq!(config3.api_key, None);

        // Mixed case variant
        let config4 = Config::from_env("GitHub-Copilot", "claude");
        assert_eq!(config4.api_key, None);
    });
}

// ============================================================================
// CompactionConfig Tests
// ============================================================================

#[test]
fn compaction_config_defaults_all_none() {
    let config = CompactionConfig::default();
    assert_eq!(config.strategy, None);
    assert_eq!(config.keep_recent, None);
    assert_eq!(config.token_budget, None);
    assert_eq!(config.proactive_threshold_pct, None);
}

#[test]
fn compaction_config_serde_roundtrip() {
    let config = CompactionConfig {
        strategy: Some(CompactionStrategy::SlidingSummary),
        keep_recent: Some(5),
        token_budget: Some(8000),
        proactive_threshold_pct: Some(0.75),
    };

    let json = serde_json::to_string(&config).expect("serialize");
    let deserialized: CompactionConfig = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(config, deserialized);
}

#[test]
fn compaction_config_validate_valid() {
    let config = CompactionConfig {
        strategy: Some(CompactionStrategy::SlidingSummary),
        keep_recent: Some(5),
        token_budget: Some(8000),
        proactive_threshold_pct: Some(0.75),
    };

    assert!(config.validate().is_ok());
}

#[test]
fn compaction_config_validate_pct_out_of_range() {
    // pct > 1.0
    let config = CompactionConfig {
        proactive_threshold_pct: Some(1.5),
        ..CompactionConfig::default()
    };
    let err = config.validate().unwrap_err();
    assert!(err.contains("proactive_threshold_pct"));

    // pct < 0.0
    let config = CompactionConfig {
        proactive_threshold_pct: Some(-0.1),
        ..CompactionConfig::default()
    };
    let err = config.validate().unwrap_err();
    assert!(err.contains("proactive_threshold_pct"));
}

#[test]
fn compaction_config_validate_zero_keep_recent() {
    let config = CompactionConfig {
        keep_recent: Some(0),
        ..CompactionConfig::default()
    };
    let err = config.validate().unwrap_err();
    assert!(err.contains("keep_recent"));
}

#[test]
fn validate_rejects_token_truncate_without_token_budget() {
    let config = CompactionConfig {
        strategy: Some(CompactionStrategy::TokenTruncate),
        token_budget: None,
        ..CompactionConfig::default()
    };
    let err = config.validate().unwrap_err();
    assert!(err.contains("token_budget"));
}

#[test]
fn validate_accepts_token_truncate_with_token_budget() {
    let config = CompactionConfig {
        strategy: Some(CompactionStrategy::TokenTruncate),
        token_budget: Some(8000),
        ..CompactionConfig::default()
    };
    assert!(config.validate().is_ok());
}

// ============================================================================
// New Tests: Copilot Provider Does Not Set API Key From Env
// ============================================================================

#[test]
#[serial]
fn copilot_provider_does_not_set_api_key_from_env() {
    // Test that copilot provider does NOT populate api_key from GITHUB_COPILOT_API_KEY
    // rig's from_env() handles environment variable resolution internally
    with_env_vars(vec![("GITHUB_COPILOT_API_KEY", "test_key")], || {
        let config = Config::from_env("copilot", "claude");

        assert_eq!(config.provider, "copilot");
        assert_eq!(config.model, "claude");
        assert_eq!(config.api_key, None);
    });
}

#[test]
#[serial]
fn github_copilot_provider_does_not_set_api_key_from_env() {
    // Test that github-copilot provider does NOT populate api_key from GITHUB_COPILOT_API_KEY
    // rig's from_env() handles environment variable resolution internally
    with_env_vars(vec![("GITHUB_COPILOT_API_KEY", "test_key")], || {
        let config = Config::from_env("github-copilot", "gpt-4o");

        assert_eq!(config.provider, "github-copilot");
        assert_eq!(config.model, "gpt-4o");
        assert_eq!(config.api_key, None);
    });
}

#[test]
#[serial]
fn non_copilot_provider_sets_api_key_from_env() {
    // Test that non-copilot providers (e.g., openai) DO populate api_key from env vars
    with_env_vars(vec![("OPENAI_API_KEY", "sk-test123")], || {
        let config = Config::from_env("openai", "gpt-4");

        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4");
        assert_eq!(config.api_key, Some("sk-test123".to_string()));
    });
}

// ---------------------------------------------------------------------------
// additional_params tests
// ---------------------------------------------------------------------------

#[test]
fn additional_params_defaults_to_none() {
    let config = Config::default();
    assert!(config.additional_params.is_none());
}

#[test]
#[serial]
fn test_session_store_env_var_sqlite() {
    with_env_vars(vec![("AGENT_SESSION_STORE_TYPE", "sqlite")], || {
        let config = Config::from_env("openai", "gpt-4");
        assert_eq!(config.session_store_type, Some(StoreType::Sqlite));
    });
}

#[test]
#[serial]
fn test_session_store_env_var_jsonl() {
    with_env_vars(vec![("AGENT_SESSION_STORE_TYPE", "jsonl")], || {
        let config = Config::from_env("openai", "gpt-4");
        assert_eq!(config.session_store_type, Some(StoreType::Jsonl));
    });
}

#[test]
#[serial]
fn test_session_store_env_var_unknown_ignored() {
    // Invalid env var values are silently ignored (None) by from_env
    with_env_vars(vec![("AGENT_SESSION_STORE_TYPE", "unknown")], || {
        let config = Config::from_env("openai", "gpt-4");
        assert_eq!(config.session_store_type, None);
    });
}

#[test]
#[serial]
fn test_session_store_env_var_not_set() {
    // When env var is not set, session_store_type should be None
    let config = Config::from_env("openai", "gpt-4");
    assert_eq!(config.session_store_type, None);
}
