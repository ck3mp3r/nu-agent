use super::*;
use nu_protocol::{Span, Value, record};
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
fn test_config_required_fields() {
    // Test that Config can be created with only required fields
    let config = Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
    };

    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4");
    assert!(config.api_key.is_none());
}

#[test]
fn test_config_all_fields() {
    // Test that Config can be created with all fields
    let config = Config {
        provider: "anthropic".to_string(),
        provider_impl: None,
        model: "claude-3-opus".to_string(),
        api_key: Some("test-key".to_string()),
        base_url: Some("https://api.example.com".to_string()),
        temperature: Some(0.7),
        max_tokens: Some(1000),
        max_context_tokens: Some(4096),
        max_output_tokens: Some(2048),
        max_tool_turns: Some(10),
    };

    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude-3-opus");
    assert_eq!(config.api_key, Some("test-key".to_string()));
    assert_eq!(config.base_url, Some("https://api.example.com".to_string()));
    assert_eq!(config.temperature, Some(0.7));
    assert_eq!(config.max_tokens, Some(1000));
    assert_eq!(config.max_context_tokens, Some(4096));
    assert_eq!(config.max_output_tokens, Some(2048));
    assert_eq!(config.max_tool_turns, Some(10));
}

#[test]
fn test_config_default_trait() {
    // Test that Default trait provides minimal defaults
    let config = Config::default();

    // Required fields should have sensible defaults
    assert_eq!(config.provider, "");
    assert_eq!(config.model, "");

    // Optional fields should be None except max_tool_turns
    assert!(config.api_key.is_none());
    assert!(config.base_url.is_none());
    assert!(config.temperature.is_none());
    assert!(config.max_tokens.is_none());
    assert!(config.max_context_tokens.is_none());
    assert!(config.max_output_tokens.is_none());

    // max_tool_turns should default to Some(20)
    assert_eq!(config.max_tool_turns, Some(20));
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
        assert_eq!(config.max_tool_turns, Some(20)); // Default
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
            assert_eq!(config.max_tool_turns, Some(20)); // Default not overridden
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
            assert!(config.max_tool_turns.is_none() || config.max_tool_turns == Some(20));
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
fn test_from_plugin_config_full() {
    // Test parsing full plugin config
    let span = Span::test_data();
    let config_value = Value::record(
        record! {
            "provider" => Value::string("openai", span),
            "model" => Value::string("gpt-4", span),
            "api_key" => Value::string("sk-plugin-test", span),
            "base_url" => Value::string("https://plugin.api.com", span),
            "temperature" => Value::float(0.9, span),
            "max_tokens" => Value::int(1500, span),
            "max_context_tokens" => Value::int(8000, span),
            "max_output_tokens" => Value::int(3000, span),
            "max_tool_turns" => Value::int(25, span),
        },
        span,
    );

    let config = Config::from_plugin_config(&config_value).unwrap();

    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4");
    assert_eq!(config.api_key, Some("sk-plugin-test".to_string()));
    assert_eq!(config.base_url, Some("https://plugin.api.com".to_string()));
    assert_eq!(config.temperature, Some(0.9));
    assert_eq!(config.max_tokens, Some(1500));
    assert_eq!(config.max_context_tokens, Some(8000));
    assert_eq!(config.max_output_tokens, Some(3000));
    assert_eq!(config.max_tool_turns, Some(25));
}

#[test]
fn test_from_plugin_config_minimal() {
    // Test with only required fields
    let span = Span::test_data();
    let config_value = Value::record(
        record! {
            "provider" => Value::string("anthropic", span),
            "model" => Value::string("claude-3-opus", span),
        },
        span,
    );

    let config = Config::from_plugin_config(&config_value).unwrap();

    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude-3-opus");
    assert!(config.api_key.is_none());
    assert!(config.base_url.is_none());
    assert_eq!(config.max_tool_turns, Some(20)); // Default
}

#[test]
fn test_from_plugin_config_partial() {
    // Test with some optional fields
    let span = Span::test_data();
    let config_value = Value::record(
        record! {
            "provider" => Value::string("openai", span),
            "model" => Value::string("gpt-3.5-turbo", span),
            "temperature" => Value::float(0.7, span),
            "max_tokens" => Value::int(2000, span),
        },
        span,
    );

    let config = Config::from_plugin_config(&config_value).unwrap();

    assert_eq!(config.temperature, Some(0.7));
    assert_eq!(config.max_tokens, Some(2000));
    assert!(config.api_key.is_none());
    assert!(config.max_context_tokens.is_none());
}

#[test]
fn test_from_plugin_config_empty_record() {
    // Test with empty record - should fail (missing required fields)
    let span = Span::test_data();
    let config_value = Value::record(record! {}, span);

    let result = Config::from_plugin_config(&config_value);
    assert!(result.is_err());
}

#[test]
fn test_from_plugin_config_not_record() {
    // Test with non-record value - should fail
    let span = Span::test_data();
    let config_value = Value::string("not a record", span);

    let result = Config::from_plugin_config(&config_value);
    assert!(result.is_err());
}

#[test]
fn test_from_plugin_config_missing_provider() {
    // Test with missing provider field
    let span = Span::test_data();
    let config_value = Value::record(
        record! {
            "model" => Value::string("gpt-4", span),
        },
        span,
    );

    let result = Config::from_plugin_config(&config_value);
    assert!(result.is_err());
}

#[test]
fn test_from_plugin_config_invalid_types() {
    // Test with invalid field types
    let span = Span::test_data();
    let config_value = Value::record(
        record! {
            "provider" => Value::string("openai", span),
            "model" => Value::string("gpt-4", span),
            "temperature" => Value::string("not-a-float", span), // Wrong type
            "max_tokens" => Value::string("not-an-int", span), // Wrong type
        },
        span,
    );

    let result = Config::from_plugin_config(&config_value);
    // Should either skip invalid fields or error - let's allow graceful skip
    if let Ok(config) = result {
        assert!(config.temperature.is_none());
        assert!(config.max_tokens.is_none());
    }
}

#[test]
fn test_merge_full_configs() {
    // Test merging two full configs - other should completely override self
    let base = Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-3.5-turbo".to_string(),
        api_key: Some("base-key".to_string()),
        base_url: Some("https://base.com".to_string()),
        temperature: Some(0.5),
        max_tokens: Some(1000),
        max_context_tokens: Some(4000),
        max_output_tokens: Some(2000),
        max_tool_turns: Some(10),
    };

    let override_config = Config {
        provider: "anthropic".to_string(),
        provider_impl: None,
        model: "claude-3-opus".to_string(),
        api_key: Some("override-key".to_string()),
        base_url: Some("https://override.com".to_string()),
        temperature: Some(0.8),
        max_tokens: Some(2000),
        max_context_tokens: Some(8000),
        max_output_tokens: Some(4000),
        max_tool_turns: Some(25),
    };

    let merged = base.merge(override_config);

    // All fields should come from override
    assert_eq!(merged.provider, "anthropic");
    assert_eq!(merged.model, "claude-3-opus");
    assert_eq!(merged.api_key, Some("override-key".to_string()));
    assert_eq!(merged.base_url, Some("https://override.com".to_string()));
    assert_eq!(merged.temperature, Some(0.8));
    assert_eq!(merged.max_tokens, Some(2000));
    assert_eq!(merged.max_context_tokens, Some(8000));
    assert_eq!(merged.max_output_tokens, Some(4000));
    assert_eq!(merged.max_tool_turns, Some(25));
}

#[test]
fn test_merge_with_partial_override() {
    // Test merging where override only has some fields
    let base = Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: Some("base-key".to_string()),
        base_url: Some("https://base.com".to_string()),
        temperature: Some(0.5),
        max_tokens: Some(1000),
        max_context_tokens: Some(4000),
        max_output_tokens: Some(2000),
        max_tool_turns: Some(10),
    };

    let override_config = Config {
        provider: "openai".to_string(), // Required, but same
        provider_impl: None,
        model: "gpt-4".to_string(), // Required, but same
        api_key: None,
        base_url: None,
        temperature: Some(0.9), // Override this
        max_tokens: Some(3000), // Override this
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
    };

    let merged = base.merge(override_config);

    // Overridden fields
    assert_eq!(merged.temperature, Some(0.9));
    assert_eq!(merged.max_tokens, Some(3000));

    // Non-overridden fields from base
    assert_eq!(merged.api_key, Some("base-key".to_string()));
    assert_eq!(merged.base_url, Some("https://base.com".to_string()));
    assert_eq!(merged.max_context_tokens, Some(4000));
    assert_eq!(merged.max_output_tokens, Some(2000));
    assert_eq!(merged.max_tool_turns, Some(10));
}

#[test]
fn test_merge_with_empty_override() {
    // Test merging with default/empty override - base should remain unchanged
    let base = Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: Some("base-key".to_string()),
        base_url: Some("https://base.com".to_string()),
        temperature: Some(0.5),
        max_tokens: Some(1000),
        max_context_tokens: Some(4000),
        max_output_tokens: Some(2000),
        max_tool_turns: Some(10),
    };

    let empty_override = Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
    };

    let merged = base.clone().merge(empty_override);

    // Should be identical to base
    assert_eq!(merged, base);
}

#[test]
fn test_merge_chain() {
    // Test chaining multiple merges
    let base = Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-3.5-turbo".to_string(),
        api_key: Some("key1".to_string()),
        base_url: None,
        temperature: Some(0.5),
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    };

    let override1 = Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(), // Override model
        api_key: None,
        base_url: Some("https://override1.com".to_string()), // Add base_url
        temperature: None,
        max_tokens: Some(2000), // Add max_tokens
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
    };

    let override2 = Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: Some("key2".to_string()), // Override api_key
        base_url: None,
        temperature: Some(0.8), // Override temperature
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
    };

    let merged = base.merge(override1).merge(override2);

    // Final state should reflect all overrides
    assert_eq!(merged.model, "gpt-4"); // From override1
    assert_eq!(merged.api_key, Some("key2".to_string())); // From override2
    assert_eq!(merged.base_url, Some("https://override1.com".to_string())); // From override1
    assert_eq!(merged.temperature, Some(0.8)); // From override2
    assert_eq!(merged.max_tokens, Some(2000)); // From override1
    assert_eq!(merged.max_tool_turns, Some(20)); // From base
}

#[test]
fn test_merge_required_fields() {
    // Test that required fields are always taken from override
    let base = Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-3.5-turbo".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
    };

    let override_config = Config {
        provider: "anthropic".to_string(),
        provider_impl: None,
        model: "claude-3-opus".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
    };

    let merged = base.merge(override_config);

    // Required fields always come from override
    assert_eq!(merged.provider, "anthropic");
    assert_eq!(merged.model, "claude-3-opus");
}

#[test]
fn test_validate_valid_config() {
    // Test that a valid config passes validation
    let config = Config {
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
    };

    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_minimal_config() {
    // Test that minimal config with only required fields passes
    let config = Config {
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
    };

    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_empty_provider() {
    // Test that empty provider fails validation
    let config = Config {
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
    };

    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_zero_max_tool_turns() {
    // Test that max_tool_turns = 0 fails
    let config = Config {
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
    };

    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_only_output_tokens_set() {
    // Test that only max_output_tokens set (no max_context_tokens) is valid
    let config = Config {
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
    };

    assert!(config.validate().is_ok());
}

// ============================================================================
// PluginConfig Tests
// ============================================================================

#[test]
fn test_plugin_config_full_structure() {
    // Test parsing a full PluginConfig structure from Nushell record
    let value = Value::test_record(record! {
        "model" => Value::test_string("openai/gpt-4"),
        "small_model" => Value::test_string("openai/gpt-3.5-turbo"),
        "providers" => Value::test_record(record! {
            "openai" => Value::test_record(record! {
                "name" => Value::test_string("OpenAI"),
                "api_key" => Value::test_string("sk-test123"),
                "base_url" => Value::test_string("https://api.openai.com/v1"),
                "models" => Value::test_record(record! {
                    "gpt-4" => Value::test_record(record! {
                        "name" => Value::test_string("GPT-4"),
                        "temperature" => Value::test_float(0.7),
                        "tool_call" => Value::test_bool(true),
                        "limit" => Value::test_record(record! {
                            "context" => Value::test_int(128000),
                            "output" => Value::test_int(4096),
                        }),
                    }),
                    "gpt-3.5-turbo" => Value::test_record(record! {
                        "limit" => Value::test_record(record! {
                            "context" => Value::test_int(16385),
                            "output" => Value::test_int(4096),
                        }),
                    }),
                }),
            }),
            "anthropic" => Value::test_record(record! {
                "api_key" => Value::test_string("sk-ant-test456"),
                "models" => Value::test_record(record! {
                    "claude-3-5-sonnet-20241022" => Value::test_record(record! {
                        "limit" => Value::test_record(record! {
                            "context" => Value::test_int(200000),
                            "output" => Value::test_int(8192),
                        }),
                    }),
                }),
            }),
        }),
    });

    let plugin_config = PluginConfig::from_plugin_config(&value).expect("should parse");

    // Verify top-level fields
    assert_eq!(plugin_config.model, "openai/gpt-4");
    assert_eq!(
        plugin_config.small_model,
        Some("openai/gpt-3.5-turbo".to_string())
    );

    // Verify OpenAI provider
    let openai = plugin_config
        .providers
        .get("openai")
        .expect("openai provider");
    assert_eq!(openai.name, Some("OpenAI".to_string()));
    assert_eq!(openai.api_key, Some("sk-test123".to_string()));
    assert_eq!(
        openai.base_url,
        Some("https://api.openai.com/v1".to_string())
    );

    // Verify OpenAI models
    let gpt4 = openai.models.get("gpt-4").expect("gpt-4 model");
    assert_eq!(gpt4.name, Some("GPT-4".to_string()));
    assert_eq!(gpt4.temperature, Some(0.7));
    assert_eq!(gpt4.tool_call, Some(true));
    assert!(gpt4.limit.is_some());
    let gpt4_limit = gpt4.limit.as_ref().unwrap();
    assert_eq!(gpt4_limit.context, Some(128000));
    assert_eq!(gpt4_limit.output, Some(4096));

    let gpt35 = openai
        .models
        .get("gpt-3.5-turbo")
        .expect("gpt-3.5-turbo model");
    assert!(gpt35.limit.is_some());
    let gpt35_limit = gpt35.limit.as_ref().unwrap();
    assert_eq!(gpt35_limit.context, Some(16385));
    assert_eq!(gpt35_limit.output, Some(4096));

    // Verify Anthropic provider
    let anthropic = plugin_config
        .providers
        .get("anthropic")
        .expect("anthropic provider");
    assert_eq!(anthropic.api_key, Some("sk-ant-test456".to_string()));

    let claude = anthropic
        .models
        .get("claude-3-5-sonnet-20241022")
        .expect("claude model");
    assert!(claude.limit.is_some());
    let claude_limit = claude.limit.as_ref().unwrap();
    assert_eq!(claude_limit.context, Some(200000));
    assert_eq!(claude_limit.output, Some(8192));
}

#[test]
fn test_plugin_config_minimal() {
    // Test parsing minimal PluginConfig (only required fields)
    let value = Value::test_record(record! {
        "model" => Value::test_string("openai/gpt-4"),
        "providers" => Value::test_record(record! {
            "openai" => Value::test_record(record! {
                "models" => Value::test_record(record! {
                    "gpt-4" => Value::test_record(record! {}),
                }),
            }),
        }),
    });

    let plugin_config = PluginConfig::from_plugin_config(&value).expect("should parse");

    assert_eq!(plugin_config.model, "openai/gpt-4");
    assert_eq!(plugin_config.small_model, None);
    assert!(plugin_config.providers.contains_key("openai"));
}

#[test]
fn test_plugin_config_missing_model() {
    // Test that missing 'model' field returns error
    let value = Value::test_record(record! {
        "providers" => Value::test_record(record! {}),
    });

    let result = PluginConfig::from_plugin_config(&value);
    assert!(result.is_err());
    let err = result.unwrap_err();
    // LabeledError's main message doesn't include field names, just check it's a meaningful error
    assert!(err.to_string().contains("Missing required field"));
}

#[test]
fn test_plugin_config_missing_providers() {
    // Test that missing 'providers' field returns error
    let value = Value::test_record(record! {
        "model" => Value::test_string("openai/gpt-4"),
    });

    let result = PluginConfig::from_plugin_config(&value);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Missing required field"));
}

#[test]
fn test_plugin_config_invalid_model_type() {
    // Test that invalid 'model' type returns error
    let value = Value::test_record(record! {
        "model" => Value::test_int(123),
        "providers" => Value::test_record(record! {}),
    });

    let result = PluginConfig::from_plugin_config(&value);
    assert!(result.is_err());
}

#[test]
fn test_plugin_config_not_record() {
    // Test that non-record value returns error
    let value = Value::test_string("not a record");

    let result = PluginConfig::from_plugin_config(&value);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Invalid plugin configuration"));
}

#[test]
fn test_plugin_config_provider_impl() {
    // Test parsing provider_impl field (for custom providers like github-copilot)
    let value = Value::test_record(record! {
        "model" => Value::test_string("copilot/claude"),
        "providers" => Value::test_record(record! {
            "copilot" => Value::test_record(record! {
                "provider_impl" => Value::test_string("openai"),
                "base_url" => Value::test_string("https://api.githubcopilot.com"),
                "models" => Value::test_record(record! {
                    "claude" => Value::test_record(record! {}),
                }),
            }),
        }),
    });

    let plugin_config = PluginConfig::from_plugin_config(&value).expect("should parse");
    let copilot = plugin_config
        .providers
        .get("copilot")
        .expect("copilot provider");
    assert_eq!(copilot.provider_impl, Some("openai".to_string()));
    assert_eq!(
        copilot.base_url,
        Some("https://api.githubcopilot.com".to_string())
    );
}

// ============================================================================
// PluginConfig::resolve_model() Tests
// ============================================================================

#[test]
fn test_resolve_model_basic() {
    // Test resolving a basic model specification
    let plugin_config = PluginConfig {
        model: "openai/gpt-4".to_string(),
        small_model: None,
        providers: {
            let mut providers = HashMap::new();
            providers.insert(
                "openai".to_string(),
                ProviderConfig {
                    name: None,
                    api_key: Some("sk-test123".to_string()),
                    base_url: None,
                    provider_impl: None,
                    models: {
                        let mut models = HashMap::new();
                        models.insert(
                            "gpt-4".to_string(),
                            ModelConfig {
                                name: None,
                                temperature: Some(0.7),
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
    };

    let config = plugin_config
        .resolve_model("openai/gpt-4")
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
        model: "anthropic/claude".to_string(),
        small_model: None,
        providers: {
            let mut providers = HashMap::new();
            providers.insert(
                "anthropic".to_string(),
                ProviderConfig {
                    name: None,
                    api_key: None, // No API key in config
                    base_url: None,
                    provider_impl: None,
                    models: HashMap::new(),
                },
            );
            providers
        },
    };

    let config = plugin_config
        .resolve_model("anthropic/claude")
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
        model: "openai/gpt-4".to_string(),
        small_model: None,
        providers: HashMap::new(),
    };

    // Missing slash
    let result = plugin_config.resolve_model("openaigpt4");
    assert!(result.is_err());

    // Too many slashes
    let result = plugin_config.resolve_model("openai/gpt/4");
    assert!(result.is_err());

    // Empty provider
    let result = plugin_config.resolve_model("/gpt-4");
    assert!(result.is_err());

    // Empty model
    let result = plugin_config.resolve_model("openai/");
    assert!(result.is_err());
}

#[test]
fn test_resolve_model_provider_not_found() {
    // Test that unknown provider returns error
    let plugin_config = PluginConfig {
        model: "openai/gpt-4".to_string(),
        small_model: None,
        providers: HashMap::new(),
    };

    let result = plugin_config.resolve_model("unknown/model");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("provider") || err.contains("not found"));
}

#[test]
fn test_resolve_model_model_not_in_config() {
    // Test that model not in provider's models map still works (uses defaults)
    let plugin_config = PluginConfig {
        model: "openai/gpt-4".to_string(),
        small_model: None,
        providers: {
            let mut providers = HashMap::new();
            providers.insert(
                "openai".to_string(),
                ProviderConfig {
                    name: None,
                    api_key: Some("sk-test123".to_string()),
                    base_url: None,
                    provider_impl: None,
                    models: HashMap::new(), // Empty models map
                },
            );
            providers
        },
    };

    let config = plugin_config
        .resolve_model("openai/gpt-3.5-turbo")
        .expect("should resolve");

    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-3.5-turbo");
    assert_eq!(config.api_key, Some("sk-test123".to_string()));
    // No model-specific config, so should use defaults
    assert_eq!(config.temperature, None);
}

#[test]
fn test_resolve_model_with_provider_impl() {
    // Test resolving with custom provider_impl (like github-copilot)
    let plugin_config = PluginConfig {
        model: "copilot/claude".to_string(),
        small_model: None,
        providers: {
            let mut providers = HashMap::new();
            providers.insert(
                "copilot".to_string(),
                ProviderConfig {
                    name: Some("GitHub Copilot".to_string()),
                    api_key: Some("ghcp-token".to_string()),
                    base_url: Some("https://api.githubcopilot.com".to_string()),
                    provider_impl: Some("openai".to_string()), // Use OpenAI API
                    models: HashMap::new(),
                },
            );
            providers
        },
    };

    let config = plugin_config
        .resolve_model("copilot/claude")
        .expect("should resolve");

    assert_eq!(config.provider, "copilot");
    assert_eq!(config.model, "claude");
    assert_eq!(config.api_key, Some("ghcp-token".to_string()));
    assert_eq!(
        config.base_url,
        Some("https://api.githubcopilot.com".to_string())
    );
}

#[test]
fn test_resolve_model_merges_limits() {
    // Test that model limits are properly merged into Config
    let plugin_config = PluginConfig {
        model: "openai/gpt-4".to_string(),
        small_model: None,
        providers: {
            let mut providers = HashMap::new();
            providers.insert(
                "openai".to_string(),
                ProviderConfig {
                    name: None,
                    api_key: None,
                    base_url: None,
                    provider_impl: None,
                    models: {
                        let mut models = HashMap::new();
                        models.insert(
                            "gpt-4".to_string(),
                            ModelConfig {
                                name: Some("GPT-4".to_string()),
                                temperature: None,
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
    };

    let config = plugin_config
        .resolve_model("openai/gpt-4")
        .expect("should resolve");

    assert_eq!(config.max_context_tokens, Some(128000));
    assert_eq!(config.max_output_tokens, Some(8192));
}

// ============================================================================
// 3-Part Format Tests (github-copilot/backend/model)
// ============================================================================

#[test]
fn resolve_model_handles_two_part_format() {
    // Test that traditional 2-part format still works (backward compatibility)
    let plugin_config = PluginConfig {
        model: "openai/gpt-4".to_string(),
        small_model: None,
        providers: {
            let mut providers = HashMap::new();
            providers.insert(
                "openai".to_string(),
                ProviderConfig {
                    name: None,
                    api_key: Some("sk-test123".to_string()),
                    base_url: None,
                    provider_impl: None,
                    models: HashMap::new(),
                },
            );
            providers
        },
    };

    let config = plugin_config
        .resolve_model("openai/gpt-4")
        .expect("should resolve 2-part format");

    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4");
    assert_eq!(config.api_key, Some("sk-test123".to_string()));
}

#[test]
fn resolve_model_handles_github_copilot_three_part() {
    // Test new 3-part format: github-copilot/anthropic/claude-sonnet-4.5
    // Provider becomes "github-copilot/anthropic", model becomes "claude-sonnet-4.5"
    let plugin_config = PluginConfig {
        model: "github-copilot/anthropic/claude-sonnet-4.5".to_string(),
        small_model: None,
        providers: {
            let mut providers = HashMap::new();
            // Provider key must be "github-copilot/anthropic" for 3-part format
            providers.insert(
                "github-copilot/anthropic".to_string(),
                ProviderConfig {
                    name: Some("GitHub Copilot (Anthropic)".to_string()),
                    api_key: Some("ghcp-token".to_string()),
                    base_url: Some("https://api.githubcopilot.com".to_string()),
                    provider_impl: Some("openai".to_string()),
                    models: HashMap::new(),
                },
            );
            providers
        },
    };

    let config = plugin_config
        .resolve_model("github-copilot/anthropic/claude-sonnet-4.5")
        .expect("should resolve 3-part format");

    assert_eq!(config.provider, "github-copilot/anthropic");
    assert_eq!(config.model, "claude-sonnet-4.5");
    assert_eq!(config.api_key, Some("ghcp-token".to_string()));
    assert_eq!(
        config.base_url,
        Some("https://api.githubcopilot.com".to_string())
    );
    assert_eq!(config.provider_impl, Some("openai".to_string()));
}

#[test]
fn resolve_model_rejects_three_part_non_github() {
    // Test that 3-part format is ONLY allowed for github-copilot
    let plugin_config = PluginConfig {
        model: "openai/foo/bar".to_string(),
        small_model: None,
        providers: {
            let mut providers = HashMap::new();
            providers.insert(
                "openai".to_string(),
                ProviderConfig {
                    name: None,
                    api_key: None,
                    base_url: None,
                    provider_impl: None,
                    models: HashMap::new(),
                },
            );
            providers
        },
    };

    let result = plugin_config.resolve_model("openai/foo/bar");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("3-part format only allowed for github-copilot"),
        "Expected error about 3-part format restriction, got: {}",
        err
    );
}

#[test]
fn resolve_model_validates_empty_parts() {
    // Test that empty parts in model specification are rejected
    let plugin_config = PluginConfig {
        model: "github-copilot//model".to_string(),
        small_model: None,
        providers: HashMap::new(),
    };

    // Empty backend (middle part)
    let result = plugin_config.resolve_model("github-copilot//model");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("empty") || err.contains("cannot be empty"),
        "Expected error about empty parts, got: {}",
        err
    );

    // Empty provider
    let result = plugin_config.resolve_model("//model");
    assert!(result.is_err());

    // Empty model at end
    let result = plugin_config.resolve_model("github-copilot/anthropic/");
    assert!(result.is_err());
}
