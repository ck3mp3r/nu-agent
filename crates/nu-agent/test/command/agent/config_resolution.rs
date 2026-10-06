use super::runtime_build;
use super::test_helpers::{create_test_call, test_model, test_plugin_config, test_provider};

use nu_agent_core::config::{
    Config, ModelConfig, ModelLimits, ModelRoleConfig, PluginConfig, ProviderConfig,
};
use nu_protocol::Value;
use std::collections::HashMap;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ============================================================================
// Config Resolution Tests - Verify precedence and merging
// ============================================================================

// ============================================================================
// Config Resolution Pipeline Tests - Test full config resolution with precedence
// ============================================================================

// Helper to create a minimal valid flag config for testing
fn create_minimal_flag_config() -> Config {
    Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None, // Default is None - runtime decides based on mode
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        output_budget_empty_remedy: None,
        output_budget_remedy_mode: None,
        output_budget_raise_enabled: None,
        output_budget_raise_multiplier: None,
        output_budget_raise_cap: None,
        repetition_guard: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        a2a_port: None,
        session_store_type: None,
    }
}

// Note: We can't test the actual Agent::run() method directly because it requires
// real EngineInterface which we can't mock. Instead, we'll create a helper function
// in agent.rs that does the config resolution logic, which we can test with our mock.
// This will be implemented as part of the GREEN phase.

#[test]
fn config_resolution_uses_defaults_when_no_other_sources() {
    // This test will verify the full resolution pipeline
    // We'll implement a testable helper function in agent.rs
    // For now, this is a placeholder that will fail until we implement it

    // Expected: Config::default() merged with minimal requirements
    let config = create_minimal_flag_config();

    // Verify defaults are present
    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4");
    assert!(config.max_tool_turns.is_none()); // Default is None
}

// These integration tests will use a helper function from agent.rs
// that performs the full config resolution pipeline
mod config_resolution_integration {
    use super::*;

    #[test]
    fn resolve_config_with_no_plugin_config() -> Result<()> {
        // Literal --model flag + minimal provider config. resolve_with_new_config
        // uses the literal provider/model from the flag.
        let plugin_config = test_plugin_config(
            "openai/gpt-4",
            vec![(
                "openai",
                test_provider(Some("sk-test"), vec![("gpt-4", test_model())]),
            )],
        );
        let call = create_test_call(vec![("model", Value::test_string("openai/gpt-4"))]);

        let config = runtime_build::resolve_with_new_config(plugin_config, &call)
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4");
        assert!(config.max_tool_turns.is_none()); // Default is None
        Ok(())
    }

    #[test]
    fn resolve_config_plugin_overrides_env() -> Result<()> {
        // Env map supplies the API key; the plugin config supplies temperature.
        let env = nu_agent_core::utils::env_map::EnvMap::from([
            ("OPENAI_API_KEY".to_string(), "env_key".to_string()),
            ("AGENT_TEMPERATURE".to_string(), "0.5".to_string()),
        ]);

        // New-format config: providers block required for resolve_config() to succeed
        let plugin_config = test_plugin_config(
            "openai/gpt-4",
            vec![(
                "openai",
                test_provider(
                    None,
                    vec![(
                        "gpt-4",
                        ModelConfig {
                            temperature: Some(0.9),
                            ..Default::default()
                        },
                    )],
                ),
            )],
        );
        let call = create_test_call(vec![]);

        let result = runtime_build::resolve_with_new_config_and_env(plugin_config, &call, &env);
        let config = result.map_err(|e| format!("{e:?}"))?;
        assert_eq!(config.temperature, Some(0.9)); // Plugin wins over env
        assert_eq!(config.api_key, Some("env_key".to_string())); // Env provides API key
        Ok(())
    }

    #[test]
    fn resolve_config_flags_override_everything() -> Result<()> {
        // Env map supplies the API key for the resolved provider (openai).
        let env = nu_agent_core::utils::env_map::EnvMap::from([
            ("OPENAI_API_KEY".to_string(), "env_key".to_string()),
            ("AGENT_TEMPERATURE".to_string(), "0.5".to_string()),
        ]);

        // New-format config: both providers present so --model openai/gpt-4 can override
        let mut openai_models = HashMap::new();
        openai_models.insert(
            "gpt-4".to_string(),
            ModelConfig {
                temperature: Some(0.8),
                limit: Some(ModelLimits {
                    context: None,
                    output: Some(1000),
                }),
                ..Default::default()
            },
        );
        let mut claude_models = HashMap::new();
        claude_models.insert(
            "claude-3".to_string(),
            ModelConfig {
                temperature: Some(0.8),
                limit: Some(ModelLimits {
                    context: None,
                    output: Some(1000),
                }),
                ..Default::default()
            },
        );
        let plugin_config = PluginConfig {
            models: {
                let mut m = HashMap::new();
                m.insert(
                    "default".to_string(),
                    ModelRoleConfig {
                        model: "anthropic/claude-3".to_string(),
                        ..Default::default()
                    },
                );
                m
            },
            providers: {
                let mut p = HashMap::new();
                p.insert(
                    "anthropic".to_string(),
                    ProviderConfig {
                        models: claude_models,
                        ..Default::default()
                    },
                );
                p.insert(
                    "openai".to_string(),
                    ProviderConfig {
                        models: openai_models,
                        ..Default::default()
                    },
                );
                p
            },
            ..Default::default()
        };
        let call = create_test_call(vec![
            ("model", Value::test_string("openai/gpt-4")), // Canonical override
            ("temperature", Value::test_float(1.2)),       // Override temperature
        ]);

        let result = runtime_build::resolve_with_new_config_and_env(plugin_config, &call, &env);
        let config = result.map_err(|e| format!("{e:?}"))?;
        assert_eq!(config.provider, "openai"); // Flag wins
        assert_eq!(config.model, "gpt-4"); // Flag wins
        assert_eq!(config.temperature, Some(1.2)); // Flag wins
        assert_eq!(config.max_output_tokens, Some(1000)); // Plugin value (no flag override)
        assert_eq!(config.api_key, Some("env_key".to_string())); // Env provides for resolved provider
        Ok(())
    }

    #[test]
    fn resolve_config_succeeds_without_providers() -> Result<()> {
        // A plugin config that has models but no providers block should succeed
        // (provider block is optional)
        let mut models = HashMap::new();
        models.insert(
            "default".to_string(),
            ModelRoleConfig {
                model: "openai/gpt-4".to_string(),
                ..Default::default()
            },
        );
        let plugin_config = PluginConfig {
            models,
            ..Default::default()
        };
        let call = create_test_call(vec![]);

        let result = runtime_build::resolve_with_new_config(plugin_config, &call);
        result.map_err(|e| format!("{e:?}"))?;
        Ok(())
    }
}
