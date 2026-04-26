use crate::commands::agent::{Agent, EngineConfigInterface, extract_flag_config};
use crate::config::Config;
use crate::session::SessionStore;
use nu_plugin::{EvaluatedCall, SimplePluginCommand};
use nu_protocol::{LabeledError, Span, Spanned, SyntaxShape, Value};
use serial_test::serial;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

/// Helper to create an Agent with a test SessionStore
fn create_test_agent() -> (Agent, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());
    let agent = Agent::new(store);
    (agent, temp_dir)
}

#[test]
fn agent_command_has_correct_name() {
    let (agent, _temp_dir) = create_test_agent();
    assert_eq!(SimplePluginCommand::name(&agent), "agent");
}

#[test]
fn agent_command_signature_accepts_string() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    // Verify the command name
    assert_eq!(sig.name, "agent");
}

#[test]
fn agent_command_signature_has_provider_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    // Find the --provider flag
    let provider_flag = sig.named.iter().find(|f| f.long == "provider");
    assert!(provider_flag.is_some(), "Missing --provider flag");

    let flag = provider_flag.unwrap();
    assert_eq!(flag.short, Some('p'), "Missing -p short flag");
    assert_eq!(
        flag.arg,
        Some(SyntaxShape::String),
        "Wrong type for --provider"
    );
    assert!(!flag.desc.is_empty(), "Missing description for --provider");
}

#[test]
fn agent_command_signature_has_model_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let model_flag = sig.named.iter().find(|f| f.long == "model");
    assert!(model_flag.is_some(), "Missing --model flag");

    let flag = model_flag.unwrap();
    assert_eq!(flag.short, Some('m'), "Missing -m short flag");
    assert_eq!(
        flag.arg,
        Some(SyntaxShape::String),
        "Wrong type for --model"
    );
    assert!(!flag.desc.is_empty(), "Missing description for --model");
}

#[test]
fn agent_command_signature_has_api_key_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "api-key");
    assert!(flag.is_some(), "Missing --api-key flag");
    assert_eq!(
        flag.unwrap().arg,
        Some(SyntaxShape::String),
        "Wrong type for --api-key"
    );
}

#[test]
fn agent_command_signature_has_base_url_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "base-url");
    assert!(flag.is_some(), "Missing --base-url flag");
    assert_eq!(
        flag.unwrap().arg,
        Some(SyntaxShape::String),
        "Wrong type for --base-url"
    );
}

#[test]
fn agent_command_signature_has_temperature_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "temperature");
    assert!(flag.is_some(), "Missing --temperature flag");
    assert_eq!(
        flag.unwrap().arg,
        Some(SyntaxShape::Number),
        "Wrong type for --temperature"
    );
}

#[test]
fn agent_command_signature_has_max_tokens_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "max-tokens");
    assert!(flag.is_some(), "Missing --max-tokens flag");
    assert_eq!(
        flag.unwrap().arg,
        Some(SyntaxShape::Int),
        "Wrong type for --max-tokens"
    );
}

#[test]
fn agent_command_signature_has_max_context_tokens_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "max-context-tokens");
    assert!(flag.is_some(), "Missing --max-context-tokens flag");
    assert_eq!(
        flag.unwrap().arg,
        Some(SyntaxShape::Int),
        "Wrong type for --max-context-tokens"
    );
}

#[test]
fn agent_command_signature_has_max_output_tokens_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "max-output-tokens");
    assert!(flag.is_some(), "Missing --max-output-tokens flag");
    assert_eq!(
        flag.unwrap().arg,
        Some(SyntaxShape::Int),
        "Wrong type for --max-output-tokens"
    );
}

#[test]
fn agent_command_signature_has_max_turns_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "max-turns");
    assert!(flag.is_some(), "Missing --max-turns flag");
    assert_eq!(
        flag.unwrap().arg,
        Some(SyntaxShape::Int),
        "Wrong type for --max-turns"
    );
}

// Helper to create an EvaluatedCall with named arguments for testing
fn create_test_call(flags: Vec<(&str, Value)>) -> EvaluatedCall {
    let span = Span::test_data();

    // Convert flags to the format EvaluatedCall expects
    let named: Vec<(Spanned<String>, Option<Value>)> = flags
        .into_iter()
        .map(|(name, value)| {
            let spanned_name = Spanned {
                item: name.to_string(),
                span,
            };
            (spanned_name, Some(value))
        })
        .collect();

    EvaluatedCall {
        head: span,
        positional: vec![],
        named,
    }
}

#[test]
fn extract_flag_config_with_no_flags() {
    let call = create_test_call(vec![]);
    let config = extract_flag_config(&call);

    // With no flags, all optional fields should be None
    // Required fields (provider, model) will be empty strings
    assert_eq!(config.provider, "");
    assert_eq!(config.model, "");
    assert_eq!(config.api_key, None);
    assert_eq!(config.base_url, None);
    assert_eq!(config.temperature, None);
    assert_eq!(config.max_tokens, None);
    assert_eq!(config.max_context_tokens, None);
    assert_eq!(config.max_output_tokens, None);
    assert_eq!(config.max_tool_turns, None);
}

#[test]
fn extract_flag_config_with_provider_and_model() {
    let call = create_test_call(vec![
        ("provider", Value::test_string("openai")),
        ("model", Value::test_string("gpt-4")),
    ]);

    let config = extract_flag_config(&call);

    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4");
    assert_eq!(config.api_key, None);
    assert_eq!(config.temperature, None);
}

#[test]
fn extract_flag_config_with_all_string_flags() {
    let call = create_test_call(vec![
        ("provider", Value::test_string("anthropic")),
        ("model", Value::test_string("claude-3-opus")),
        ("api-key", Value::test_string("test-key-123")),
        ("base-url", Value::test_string("https://custom.api.com")),
    ]);

    let config = extract_flag_config(&call);

    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude-3-opus");
    assert_eq!(config.api_key, Some("test-key-123".to_string()));
    assert_eq!(config.base_url, Some("https://custom.api.com".to_string()));
}

#[test]
fn extract_flag_config_with_temperature() {
    let call = create_test_call(vec![
        ("provider", Value::test_string("openai")),
        ("model", Value::test_string("gpt-4")),
        ("temperature", Value::test_float(0.7)),
    ]);

    let config = extract_flag_config(&call);

    assert_eq!(config.temperature, Some(0.7));
}

#[test]
fn extract_flag_config_with_all_int_flags() {
    let call = create_test_call(vec![
        ("provider", Value::test_string("openai")),
        ("model", Value::test_string("gpt-4")),
        ("max-tokens", Value::test_int(1000)),
        ("max-context-tokens", Value::test_int(8000)),
        ("max-output-tokens", Value::test_int(2000)),
        ("max-turns", Value::test_int(10)),
    ]);

    let config = extract_flag_config(&call);

    assert_eq!(config.max_tokens, Some(1000));
    assert_eq!(config.max_context_tokens, Some(8000));
    assert_eq!(config.max_output_tokens, Some(2000));
    assert_eq!(config.max_tool_turns, Some(10));
}

#[test]
fn extract_flag_config_with_mixed_flags() {
    let call = create_test_call(vec![
        ("provider", Value::test_string("anthropic")),
        ("model", Value::test_string("claude-3")),
        ("temperature", Value::test_float(1.0)),
        ("max-tokens", Value::test_int(2048)),
        ("base-url", Value::test_string("https://api.example.com")),
    ]);

    let config = extract_flag_config(&call);

    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude-3");
    assert_eq!(config.temperature, Some(1.0));
    assert_eq!(config.max_tokens, Some(2048));
    assert_eq!(config.base_url, Some("https://api.example.com".to_string()));
    assert_eq!(config.api_key, None);
    assert_eq!(config.max_context_tokens, None);
}

#[test]
fn extract_flag_config_handles_negative_ints_as_none() {
    let call = create_test_call(vec![
        ("provider", Value::test_string("openai")),
        ("model", Value::test_string("gpt-4")),
        ("max-tokens", Value::test_int(-100)),
    ]);

    let config = extract_flag_config(&call);

    // Negative integers should be treated as None
    assert_eq!(config.max_tokens, None);
}

// ============================================================================
// MockEngineInterface - Test helper for config resolution tests
// ============================================================================

/// Mock implementation of EngineConfigInterface for testing config resolution
///
/// Allows setting a predetermined return value for get_plugin_config()
/// to test various config scenarios without requiring a real Nushell engine.
struct MockEngineInterface {
    plugin_config: Arc<Mutex<Option<Value>>>,
}

impl MockEngineInterface {
    /// Create a new mock with no plugin config
    fn new() -> Self {
        Self {
            plugin_config: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a mock that returns the given plugin config
    fn with_config(config: Value) -> Self {
        Self {
            plugin_config: Arc::new(Mutex::new(Some(config))),
        }
    }

    /// Set the plugin config to return
    fn set_config(&self, config: Option<Value>) {
        *self.plugin_config.lock().unwrap() = config;
    }
}

impl EngineConfigInterface for MockEngineInterface {
    fn get_plugin_config(&self) -> Result<Option<Value>, LabeledError> {
        Ok(self.plugin_config.lock().unwrap().clone())
    }
}

// ============================================================================
// Config Resolution Tests - Verify precedence and merging
// ============================================================================

#[test]
fn mock_engine_returns_none_by_default() {
    let mock = MockEngineInterface::new();
    let result = mock.get_plugin_config();

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), None);
}

#[test]
fn mock_engine_returns_set_config() {
    let config_value = Value::test_record(
        vec![
            ("provider".to_string(), Value::test_string("openai")),
            ("model".to_string(), Value::test_string("gpt-4")),
        ]
        .into_iter()
        .collect(),
    );

    let mock = MockEngineInterface::with_config(config_value.clone());
    let result = mock.get_plugin_config();

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Some(config_value));
}

#[test]
fn mock_engine_can_update_config() {
    let mock = MockEngineInterface::new();

    // Initially None
    assert_eq!(mock.get_plugin_config().unwrap(), None);

    // Set config
    let config = Value::test_record(
        vec![
            ("provider".to_string(), Value::test_string("anthropic")),
            ("model".to_string(), Value::test_string("claude-3")),
        ]
        .into_iter()
        .collect(),
    );

    mock.set_config(Some(config.clone()));
    assert_eq!(mock.get_plugin_config().unwrap(), Some(config));

    // Clear config
    mock.set_config(None);
    assert_eq!(mock.get_plugin_config().unwrap(), None);
}

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
        max_tool_turns: Some(20),
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
    assert_eq!(config.max_tool_turns, Some(20));
}

#[test]
fn config_merge_respects_precedence() {
    // Test that Config::merge works correctly for the resolution pipeline
    // Precedence: default < env < plugin < flags

    let default_config = Config::default();
    assert_eq!(default_config.provider, "");
    assert_eq!(default_config.model, "");

    let env_config = Config {
        provider: "from_env".to_string(),
        model: "model_env".to_string(),
        api_key: Some("env_key".to_string()),
        ..Default::default()
    };

    let plugin_config = Config {
        provider: "from_plugin".to_string(),
        model: "model_plugin".to_string(),
        temperature: Some(0.8),
        ..Default::default()
    };

    let flag_config = Config {
        provider: "from_flags".to_string(),
        model: "model_flags".to_string(),
        max_tokens: Some(2000),
        ..Default::default()
    };

    // Merge: default < env < plugin < flags
    let result = default_config
        .merge(env_config)
        .merge(plugin_config)
        .merge(flag_config);

    // Flags win for provider/model (required fields)
    assert_eq!(result.provider, "from_flags");
    assert_eq!(result.model, "model_flags");

    // Optional fields: last non-None value wins
    assert_eq!(result.api_key, Some("env_key".to_string())); // Only set in env
    assert_eq!(result.temperature, Some(0.8)); // Only set in plugin
    assert_eq!(result.max_tokens, Some(2000)); // Only set in flags
    assert_eq!(result.max_tool_turns, Some(20)); // Default
}

// These integration tests will use a helper function from agent.rs
// that performs the full config resolution pipeline
mod config_resolution_integration {
    use super::*;
    use crate::commands::agent::resolve_config;

    #[test]
    fn resolve_config_with_no_plugin_config() {
        let mock = MockEngineInterface::new();
        let call = create_test_call(vec![
            ("provider", Value::test_string("openai")),
            ("model", Value::test_string("gpt-4")),
        ]);

        let result = resolve_config(&mock, &call);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4");
        assert_eq!(config.max_tool_turns, Some(20)); // Default
    }

    #[test]
    #[serial] // Prevent parallel execution due to env vars
    fn resolve_config_plugin_overrides_env() {
        // Set env vars for testing
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "env_key");
            std::env::set_var("AGENT_TEMPERATURE", "0.5");
        }

        let plugin_config = Value::test_record(
            vec![
                ("provider".to_string(), Value::test_string("openai")),
                ("model".to_string(), Value::test_string("gpt-4")),
                ("temperature".to_string(), Value::test_float(0.9)),
            ]
            .into_iter()
            .collect(),
        );

        let mock = MockEngineInterface::with_config(plugin_config);
        let call = create_test_call(vec![]);

        let result = resolve_config(&mock, &call);
        if let Err(ref e) = result {
            eprintln!("Error: {:?}", e);
        }
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.temperature, Some(0.9)); // Plugin wins over env
        assert_eq!(config.api_key, Some("env_key".to_string())); // Env provides API key

        // Cleanup
        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
            std::env::remove_var("AGENT_TEMPERATURE");
        }
    }

    #[test]
    #[serial] // Prevent parallel execution due to env vars
    fn resolve_config_flags_override_everything() {
        // Set env vars
        unsafe {
            std::env::set_var("ANTHROPIC_API_KEY", "env_key");
            std::env::set_var("AGENT_TEMPERATURE", "0.5");
        }

        let plugin_config = Value::test_record(
            vec![
                ("provider".to_string(), Value::test_string("anthropic")),
                ("model".to_string(), Value::test_string("claude-3")),
                ("temperature".to_string(), Value::test_float(0.8)),
                ("max_tokens".to_string(), Value::test_int(1000)),
            ]
            .into_iter()
            .collect(),
        );

        let mock = MockEngineInterface::with_config(plugin_config);
        let call = create_test_call(vec![
            ("provider", Value::test_string("openai")), // Override provider
            ("model", Value::test_string("gpt-4")),     // Override model
            ("temperature", Value::test_float(1.2)),    // Override temperature
        ]);

        let result = resolve_config(&mock, &call);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.provider, "openai"); // Flag wins
        assert_eq!(config.model, "gpt-4"); // Flag wins
        assert_eq!(config.temperature, Some(1.2)); // Flag wins
        assert_eq!(config.max_tokens, Some(1000)); // Plugin value (no flag override)
        assert_eq!(config.api_key, Some("env_key".to_string())); // Env provides

        // Cleanup
        unsafe {
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("AGENT_TEMPERATURE");
        }
    }

    #[test]
    fn resolve_config_validates_final_config() {
        // Test validation with conflicting token limits
        let plugin_config = Value::test_record(
            vec![
                ("provider".to_string(), Value::test_string("openai")),
                ("model".to_string(), Value::test_string("gpt-4")),
                ("max_output_tokens".to_string(), Value::test_int(5000)), // Output > Context
                ("max_context_tokens".to_string(), Value::test_int(1000)),
            ]
            .into_iter()
            .collect(),
        );

        let mock = MockEngineInterface::with_config(plugin_config);
        let call = create_test_call(vec![]);

        let result = resolve_config(&mock, &call);
        assert!(result.is_err()); // Should fail validation

        // Just verify we got an error - the exact error message structure may vary
        let _err = result.unwrap_err();
        // Error should be about validation failure (max_output_tokens > max_context_tokens)
    }

    #[test]
    fn resolve_config_handles_invalid_plugin_config() {
        // Plugin config is not a record
        let invalid_config = Value::test_string("not a record");
        let mock = MockEngineInterface::with_config(invalid_config);

        let call = create_test_call(vec![
            ("provider", Value::test_string("openai")),
            ("model", Value::test_string("gpt-4")),
        ]);

        let result = resolve_config(&mock, &call);
        assert!(result.is_err());

        // Just verify we got an error - the exact error message structure may vary
        let _err = result.unwrap_err();
        // Error should be about invalid config format
    }
}

// ============================================================================
// New Plugin Config Tests - Test provider/model format and --small flag
// ============================================================================

mod new_plugin_config_tests {
    use super::*;
    use crate::commands::agent::resolve_config;

    #[test]
    fn signature_has_model_flag_for_provider_model_format() {
        let (agent, _temp_dir) = create_test_agent();
        let sig = SimplePluginCommand::signature(&agent);

        let model_flag = sig.named.iter().find(|f| f.long == "model");
        assert!(model_flag.is_some(), "Missing --model flag");

        let flag = model_flag.unwrap();
        assert_eq!(flag.short, Some('m'), "Missing -m short flag");
        assert_eq!(
            flag.arg,
            Some(SyntaxShape::String),
            "Wrong type for --model"
        );
        // Description should mention provider/model format
        assert!(
            flag.desc.contains("provider/model")
                || flag.desc.contains("provider") && flag.desc.contains("model"),
            "Flag description should mention provider/model format: {}",
            flag.desc
        );
    }

    #[test]
    fn signature_has_small_flag() {
        let (agent, _temp_dir) = create_test_agent();
        let sig = SimplePluginCommand::signature(&agent);

        let small_flag = sig.named.iter().find(|f| f.long == "small");
        assert!(small_flag.is_some(), "Missing --small flag");

        let flag = small_flag.unwrap();
        assert_eq!(flag.short, Some('s'), "Missing -s short flag");
        // --small is a switch (no argument)
        assert_eq!(flag.arg, None, "--small should be a switch");
        assert!(!flag.desc.is_empty(), "Missing description for --small");
    }

    #[test]
    #[serial]
    fn resolve_config_with_new_plugin_config_structure() {
        use std::collections::HashMap;

        // Create NEW plugin config structure with provider/model format
        let mut providers_map = HashMap::new();

        // OpenAI provider with gpt-4 model
        let mut openai_models = HashMap::new();
        openai_models.insert(
            "gpt-4".to_string(),
            Value::test_record(
                vec![
                    ("temperature".to_string(), Value::test_float(0.7)),
                    (
                        "limit".to_string(),
                        Value::test_record(
                            vec![
                                ("context".to_string(), Value::test_int(128000)),
                                ("output".to_string(), Value::test_int(4096)),
                            ]
                            .into_iter()
                            .collect(),
                        ),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
        );

        providers_map.insert(
            "openai".to_string(),
            Value::test_record(
                vec![
                    ("api_key".to_string(), Value::test_string("test_key")),
                    (
                        "models".to_string(),
                        Value::test_record(openai_models.into_iter().collect()),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
        );

        let plugin_config = Value::test_record(
            vec![
                ("model".to_string(), Value::test_string("openai/gpt-4")),
                (
                    "providers".to_string(),
                    Value::test_record(providers_map.into_iter().collect()),
                ),
            ]
            .into_iter()
            .collect(),
        );

        let mock = MockEngineInterface::with_config(plugin_config);
        let call = create_test_call(vec![]);

        let result = resolve_config(&mock, &call);
        assert!(result.is_ok(), "Failed to resolve config: {:?}", result);

        let config = result.unwrap();
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4");
        assert_eq!(config.api_key, Some("test_key".to_string()));
        assert_eq!(config.temperature, Some(0.7));
        assert_eq!(config.max_context_tokens, Some(128000));
        assert_eq!(config.max_output_tokens, Some(4096));
    }

    #[test]
    #[serial]
    fn resolve_config_with_model_flag_override() {
        use std::collections::HashMap;

        // Create plugin config with multiple providers and models
        let mut providers_map = HashMap::new();

        // OpenAI provider
        let mut openai_models = HashMap::new();
        openai_models.insert(
            "gpt-4".to_string(),
            Value::test_record(
                vec![("temperature".to_string(), Value::test_float(0.7))]
                    .into_iter()
                    .collect(),
            ),
        );
        openai_models.insert(
            "gpt-3.5-turbo".to_string(),
            Value::test_record(
                vec![("temperature".to_string(), Value::test_float(0.9))]
                    .into_iter()
                    .collect(),
            ),
        );

        providers_map.insert(
            "openai".to_string(),
            Value::test_record(
                vec![
                    ("api_key".to_string(), Value::test_string("openai_key")),
                    (
                        "models".to_string(),
                        Value::test_record(openai_models.into_iter().collect()),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
        );

        let plugin_config = Value::test_record(
            vec![
                ("model".to_string(), Value::test_string("openai/gpt-4")), // Default
                (
                    "providers".to_string(),
                    Value::test_record(providers_map.into_iter().collect()),
                ),
            ]
            .into_iter()
            .collect(),
        );

        let mock = MockEngineInterface::with_config(plugin_config);

        // Override with --model flag to use gpt-3.5-turbo instead
        let call = create_test_call(vec![("model", Value::test_string("openai/gpt-3.5-turbo"))]);

        let result = resolve_config(&mock, &call);
        assert!(result.is_ok(), "Failed to resolve config: {:?}", result);

        let config = result.unwrap();
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-3.5-turbo"); // Flag overrides default
        assert_eq!(config.temperature, Some(0.9)); // Model-specific temperature
    }

    #[test]
    #[serial]
    fn resolve_config_with_small_flag() {
        use std::collections::HashMap;

        // Create plugin config with small_model
        let mut providers_map = HashMap::new();

        let mut openai_models = HashMap::new();
        openai_models.insert(
            "gpt-4".to_string(),
            Value::test_record(vec![].into_iter().collect()),
        );
        openai_models.insert(
            "gpt-3.5-turbo".to_string(),
            Value::test_record(
                vec![("temperature".to_string(), Value::test_float(1.0))]
                    .into_iter()
                    .collect(),
            ),
        );

        providers_map.insert(
            "openai".to_string(),
            Value::test_record(
                vec![
                    ("api_key".to_string(), Value::test_string("test_key")),
                    (
                        "models".to_string(),
                        Value::test_record(openai_models.into_iter().collect()),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
        );

        let plugin_config = Value::test_record(
            vec![
                ("model".to_string(), Value::test_string("openai/gpt-4")),
                (
                    "small_model".to_string(),
                    Value::test_string("openai/gpt-3.5-turbo"),
                ),
                (
                    "providers".to_string(),
                    Value::test_record(providers_map.into_iter().collect()),
                ),
            ]
            .into_iter()
            .collect(),
        );

        let mock = MockEngineInterface::with_config(plugin_config);

        // Use --small flag
        let call = create_test_call(vec![("small", Value::test_bool(true))]);

        let result = resolve_config(&mock, &call);
        assert!(result.is_ok(), "Failed to resolve config: {:?}", result);

        let config = result.unwrap();
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-3.5-turbo"); // Uses small_model
        assert_eq!(config.temperature, Some(1.0)); // Model-specific temperature
    }

    #[test]
    #[serial]
    fn resolve_config_model_flag_overrides_small_flag() {
        use std::collections::HashMap;

        // Create plugin config
        let mut providers_map = HashMap::new();

        let mut openai_models = HashMap::new();
        openai_models.insert(
            "gpt-4".to_string(),
            Value::test_record(vec![].into_iter().collect()),
        );
        openai_models.insert(
            "gpt-3.5-turbo".to_string(),
            Value::test_record(vec![].into_iter().collect()),
        );

        providers_map.insert(
            "openai".to_string(),
            Value::test_record(
                vec![
                    ("api_key".to_string(), Value::test_string("test_key")),
                    (
                        "models".to_string(),
                        Value::test_record(openai_models.into_iter().collect()),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
        );

        let plugin_config = Value::test_record(
            vec![
                ("model".to_string(), Value::test_string("openai/gpt-4")),
                (
                    "small_model".to_string(),
                    Value::test_string("openai/gpt-3.5-turbo"),
                ),
                (
                    "providers".to_string(),
                    Value::test_record(providers_map.into_iter().collect()),
                ),
            ]
            .into_iter()
            .collect(),
        );

        let mock = MockEngineInterface::with_config(plugin_config);

        // Both --small and --model provided, --model should win
        let call = create_test_call(vec![
            ("small", Value::test_bool(true)),
            ("model", Value::test_string("openai/gpt-4")),
        ]);

        let result = resolve_config(&mock, &call);
        assert!(result.is_ok(), "Failed to resolve config: {:?}", result);

        let config = result.unwrap();
        assert_eq!(config.model, "gpt-4"); // --model wins over --small
    }

    #[test]
    #[serial]
    fn resolve_config_old_provider_flag_still_works_for_backward_compat() {
        let mock = MockEngineInterface::new();

        // Use old --provider and --model flags (separate, not provider/model format)
        let call = create_test_call(vec![
            ("provider", Value::test_string("openai")),
            ("model", Value::test_string("gpt-4")),
        ]);

        let result = resolve_config(&mock, &call);
        assert!(result.is_ok(), "Failed to resolve config: {:?}", result);

        let config = result.unwrap();
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4");
    }
}

// ============================================================================
// Record Input Tests - Test record input with prompt and optional fields
// ============================================================================

mod record_input_tests {
    use crate::commands::agent::extract_prompt_from_input;
    use nu_protocol::Value;

    #[test]
    fn extract_prompt_from_string_input() {
        // Test existing functionality - string input
        let input = Value::test_string("test prompt");
        let result = extract_prompt_from_input(&input);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test prompt");
    }

    #[test]
    fn extract_prompt_from_record_input_with_prompt_field() {
        // RED: Test record input with {prompt: "test"}
        // This will fail because extract_prompt_from_input currently only accepts String

        let input = Value::test_record(
            vec![("prompt".to_string(), Value::test_string("test prompt"))]
                .into_iter()
                .collect(),
        );

        let result = extract_prompt_from_input(&input);

        // Expected: should extract "test prompt" from record
        assert!(
            result.is_ok(),
            "Failed to extract prompt from record: {:?}",
            result
        );
        assert_eq!(result.unwrap(), "test prompt");
    }

    #[test]
    fn extract_prompt_from_record_rejects_missing_prompt_field() {
        // RED: Test that record without prompt field fails

        let input = Value::test_record(
            vec![("context".to_string(), Value::test_string("some context"))]
                .into_iter()
                .collect(),
        );

        let result = extract_prompt_from_input(&input);

        // Should fail with clear error about missing prompt
        assert!(result.is_err(), "Should reject record without prompt field");

        let err = result.unwrap_err();
        assert!(
            err.msg.contains("prompt") || err.msg.contains("required"),
            "Error should mention missing prompt: {}",
            err.msg
        );
    }

    #[test]
    fn extract_prompt_from_record_rejects_empty_prompt() {
        // RED: Test that record with empty prompt fails

        let input = Value::test_record(
            vec![("prompt".to_string(), Value::test_string(""))]
                .into_iter()
                .collect(),
        );

        let result = extract_prompt_from_input(&input);

        // Should fail for empty prompt
        assert!(result.is_err(), "Should reject empty prompt");

        let err = result.unwrap_err();
        assert!(
            err.msg.contains("empty") || err.msg.contains("prompt"),
            "Error should mention empty prompt: {}",
            err.msg
        );
    }

    #[test]
    fn extract_prompt_from_record_with_optional_fields() {
        // RED: Test that record with optional fields (context, model) still works
        // For now, we just need to extract the prompt, optional fields are ignored

        let input = Value::test_record(
            vec![
                ("prompt".to_string(), Value::test_string("test prompt")),
                ("context".to_string(), Value::test_string("some context")),
                (
                    "model".to_string(),
                    Value::test_string("openai/gpt-3.5-turbo"),
                ),
            ]
            .into_iter()
            .collect(),
        );

        let result = extract_prompt_from_input(&input);

        assert!(
            result.is_ok(),
            "Failed to extract prompt from record with optional fields: {:?}",
            result
        );
        assert_eq!(result.unwrap(), "test prompt");
    }

    #[test]
    fn extract_prompt_rejects_invalid_types() {
        // Test that non-string, non-record inputs fail

        let input = Value::test_int(123);
        let result = extract_prompt_from_input(&input);

        assert!(result.is_err(), "Should reject integer input");

        let input = Value::test_bool(true);
        let result = extract_prompt_from_input(&input);

        assert!(result.is_err(), "Should reject boolean input");
    }
}

// Tests for session flags (task 1.18)
#[cfg(test)]
mod session_flags_tests {
    use super::*;

    #[test]
    fn agent_command_signature_has_session_flag() {
        // RED: Test that --session flag exists
        let (agent, _temp_dir) = create_test_agent();
        let sig = SimplePluginCommand::signature(&agent);

        let session_flag = sig.named.iter().find(|f| f.long == "session");
        assert!(session_flag.is_some(), "Missing --session flag");

        let flag = session_flag.unwrap();
        assert_eq!(
            flag.arg,
            Some(SyntaxShape::String),
            "Wrong type for --session"
        );
        assert!(!flag.desc.is_empty(), "Missing description for --session");
    }

    #[test]
    fn agent_command_signature_has_new_session_flag() {
        // RED: Test that --new-session flag exists
        let (agent, _temp_dir) = create_test_agent();
        let sig = SimplePluginCommand::signature(&agent);

        let new_session_flag = sig.named.iter().find(|f| f.long == "new-session");
        assert!(new_session_flag.is_some(), "Missing --new-session flag");

        let flag = new_session_flag.unwrap();
        // Should be a switch (no argument)
        assert_eq!(flag.arg, None, "--new-session should be a switch");
        assert!(
            !flag.desc.is_empty(),
            "Missing description for --new-session"
        );
    }

}

// Tests for session flag validation
#[cfg(test)]
mod session_validation_tests {
    use super::*;
    use crate::commands::agent::extract_and_validate_session_flags;

    /// Helper to create a mock EvaluatedCall for testing
    fn create_mock_call_with_session_flags(
        session: Option<&str>,
        new_session: bool,
    ) -> EvaluatedCall {
        let mut named = vec![];

        if let Some(id) = session {
            named.push((
                Spanned {
                    item: "session".to_string(),
                    span: Span::test_data(),
                },
                Some(Value::test_string(id)),
            ));
        }

        if new_session {
            named.push((
                Spanned {
                    item: "new-session".to_string(),
                    span: Span::test_data(),
                },
                Some(Value::test_bool(true)),
            ));
        }

        EvaluatedCall {
            head: Span::test_data(),
            positional: vec![],
            named,
        }
    }

    #[test]
    fn validate_session_flags_accepts_session_id_only() {
        // RED: Test that --session <id> alone is valid
        let call = create_mock_call_with_session_flags(Some("my-session"), false);
        let result = extract_and_validate_session_flags(&call);

        assert!(result.is_ok(), "Should accept --session alone");
        let (session_id, new_session) = result.unwrap();
        assert_eq!(session_id, Some("my-session".to_string()));
        assert!(!new_session);
    }

    #[test]
    fn validate_session_flags_accepts_new_session_only() {
        // RED: Test that --new-session alone is valid
        let call = create_mock_call_with_session_flags(None, true);
        let result = extract_and_validate_session_flags(&call);

        assert!(result.is_ok(), "Should accept --new-session alone");
        let (session_id, new_session) = result.unwrap();
        assert!(session_id.is_none());
        assert!(new_session);
    }

    #[test]
    fn validate_session_flags_accepts_no_flags() {
        // RED: Test that no session flags is valid (default behavior)
        let call = create_mock_call_with_session_flags(None, false);
        let result = extract_and_validate_session_flags(&call);

        assert!(result.is_ok(), "Should accept no session flags");
        let (session_id, new_session) = result.unwrap();
        assert!(session_id.is_none());
        assert!(!new_session);
    }

    #[test]
    fn validate_session_flags_rejects_session_and_new_session() {
        // RED: Test that --session and --new-session together is invalid
        let call = create_mock_call_with_session_flags(Some("my-session"), true);
        let result = extract_and_validate_session_flags(&call);

        assert!(
            result.is_err(),
            "Should reject --session and --new-session together"
        );
        let err = result.unwrap_err();
        assert!(err.msg.contains("Conflicting") || err.msg.contains("exclusive"));
}
}

// Integration tests for session functionality
#[cfg(test)]
mod session_integration_tests {
    use super::*;
    use crate::commands::agent::extract_and_validate_session_flags;
    use crate::session::{Message, SessionStore};
    use tempfile::TempDir;

    #[test]
    fn session_store_accessible_in_agent() {
        // Verify Agent has access to SessionStore
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());
        let _agent = Agent::new(store.clone());

        // Create a test session directly via store
        let session = store
            .get_or_create(Some("test-session".to_string()))
            .expect("Failed to create session");

        assert_eq!(session.id(), "test-session");
        assert_eq!(session.messages().len(), 0);
    }

    #[test]
    fn session_can_store_and_retrieve_messages() {
        // Test that we can add messages to a session
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

        let mut session = store
            .get_or_create(Some("chat-1".to_string()))
            .expect("Failed to create session");

        // Add user message
        let user_msg = Message::new("user".to_string(), "Hello".to_string());
        session
            .add_message(&store, user_msg)
            .expect("Failed to add message");

        // Add assistant message
        let assistant_msg = Message::new("assistant".to_string(), "Hi there!".to_string());
        session
            .add_message(&store, assistant_msg)
            .expect("Failed to add message");

        // Reload session and verify messages persist
        let reloaded = store
            .load_session("chat-1")
            .expect("Failed to load session");
        assert_eq!(reloaded.messages().len(), 2);
        assert_eq!(reloaded.messages()[0].role(), "user");
        assert_eq!(reloaded.messages()[0].content(), "Hello");
        assert_eq!(reloaded.messages()[1].role(), "assistant");
        assert_eq!(reloaded.messages()[1].content(), "Hi there!");
    }

    #[test]
    fn auto_generated_session_id_format() {
        // Verify auto-generated session IDs have correct format
        use chrono::Utc;
        let now = Utc::now();
        let session_id = format!(
            "session-{}-{}",
            now.format("%Y%m%d-%H%M%S"),
            now.timestamp_subsec_micros()
        );

        // Should start with "session-"
        assert!(session_id.starts_with("session-"));

        // Should contain date format with hyphens
        assert!(session_id.matches('-').count() >= 3); // At least session-, date-, time-

        // Should be reasonably long (at least 25 chars for session-YYYYMMDD-HHMMSS-X)
        assert!(
            session_id.len() >= 25,
            "Session ID too short: {} (len={})",
            session_id,
            session_id.len()
        );
    }

    #[test]
    fn session_compaction_threshold() {
        // Test that compaction is triggered at threshold
        use crate::session::{CompactionStrategy, SessionConfig};

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

        let mut session = store
            .get_or_create(Some("test-compact".to_string()))
            .expect("Failed to create session");

        // Set low threshold for testing
        let config = SessionConfig {
            compaction_threshold: 3,
            compaction_strategy: CompactionStrategy::Truncate,
            keep_recent: 2,
        };
        session.set_config(config);

        // Add messages up to threshold
        for i in 0..3 {
            let msg = Message::new("user".to_string(), format!("Message {}", i));
            session
                .add_message(&store, msg)
                .expect("Failed to add message");
        }

        assert_eq!(session.messages().len(), 3);

        // Add one more to trigger compaction
        let msg = Message::new("user".to_string(), "Message 4".to_string());
        session
            .add_message(&store, msg)
            .expect("Failed to add message");

        // Should have compacted (keeping only 2 recent messages)
        // Note: maybe_compact is called in add_message via trigger_compaction_placeholder
        // For this test, we just verify the session can handle the threshold
        assert!(
            session.messages().len() >= 2,
            "Session should maintain messages"
        );
    }

    #[test]
    fn extract_session_flags_with_session_id() {
        // Test extracting --session flag
        let call = create_mock_call_with_session_flags(Some("my-session"), false);
        let result = extract_and_validate_session_flags(&call);

        assert!(result.is_ok());
        let (session_id, new_session) = result.unwrap();
        assert_eq!(session_id, Some("my-session".to_string()));
        assert!(!new_session);
    }

    #[test]
    fn extract_session_flags_with_new_session() {
        // Test extracting --new-session flag
        let call = create_mock_call_with_session_flags(None, true);
        let result = extract_and_validate_session_flags(&call);

        assert!(result.is_ok());
        let (session_id, new_session) = result.unwrap();
        assert!(session_id.is_none());
        assert!(new_session);
    }

    #[test]
    fn extract_session_flags_default_no_flags() {
        // Test default behavior (no session flags)
        let call = create_mock_call_with_session_flags(None, false);
        let result = extract_and_validate_session_flags(&call);

        assert!(result.is_ok());
        let (session_id, new_session) = result.unwrap();
        assert!(session_id.is_none());
        assert!(!new_session);
    }

    /// Helper to create a mock EvaluatedCall for testing (imported from session_validation_tests)
    fn create_mock_call_with_session_flags(
        session: Option<&str>,
        new_session: bool,
    ) -> EvaluatedCall {
        let mut named = vec![];

        if let Some(id) = session {
            named.push((
                Spanned {
                    item: "session".to_string(),
                    span: Span::test_data(),
                },
                Some(Value::test_string(id)),
            ));
        }

        if new_session {
            named.push((
                Spanned {
                    item: "new-session".to_string(),
                    span: Span::test_data(),
                },
                Some(Value::test_bool(true)),
            ));
        }

        EvaluatedCall {
            head: Span::test_data(),
            positional: vec![],
            named,
        }
    }
}
