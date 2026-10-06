//! `Config::from_env()` tests.

use crate::config::Config;
use crate::test_support::env_map;
use crate::utils::env_map::EnvMap;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_from_env_with_provider_api_key() {
    // Test reading provider-specific API key from environment
    let env = env_map(&[("OPENAI_API_KEY", "sk-test123")]);
    let config = Config::from_env(&env, "openai", "gpt-4");

    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4");
    assert_eq!(config.api_key, Some("sk-test123".to_string()));
    assert!(config.max_tool_turns.is_none()); // Default is now None
}

#[test]
fn test_from_env_missing_api_key() {
    // Test that missing API key results in None (graceful handling)
    let env = EnvMap::new();
    let config = Config::from_env(&env, "nonexistent", "model-1");

    assert_eq!(config.provider, "nonexistent");
    assert_eq!(config.model, "model-1");
    assert!(config.api_key.is_none());
}

#[test]
fn test_from_env_with_agent_overrides() {
    // Test AGENT_* environment variable overrides
    let env = env_map(&[
        ("ANTHROPIC_API_KEY", "sk-ant-test"),
        ("AGENT_TEMPERATURE", "0.8"),
        ("AGENT_MAX_TOKENS", "2000"),
        ("AGENT_MAX_CONTEXT_TOKENS", "8192"),
        ("AGENT_MAX_OUTPUT_TOKENS", "4096"),
        ("AGENT_MAX_TOOL_TURNS", "15"),
        ("AGENT_BASE_URL", "https://custom.api.com"),
        ("AGENT_MAX_TOOL_RESULT_BYTES", "15000"),
    ]);
    let config = Config::from_env(&env, "anthropic", "claude-3-opus");

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
}

#[test]
fn test_from_env_partial_overrides() {
    // Test with only some AGENT_* vars set
    let env = env_map(&[
        ("OPENAI_API_KEY", "sk-partial"),
        ("AGENT_TEMPERATURE", "0.5"),
    ]);
    let config = Config::from_env(&env, "openai", "gpt-3.5-turbo");

    assert_eq!(config.temperature, Some(0.5));
    assert!(config.max_tokens.is_none());
    assert!(config.base_url.is_none());
    assert!(config.max_tool_turns.is_none()); // Default is None, not overridden
}

#[test]
fn test_from_env_invalid_numeric_values() {
    // Test that invalid numeric values are ignored (None)
    let env = env_map(&[
        ("AGENT_TEMPERATURE", "not-a-number"),
        ("AGENT_MAX_TOKENS", "invalid"),
        ("AGENT_MAX_TOOL_TURNS", "-5"),
    ]);
    let config = Config::from_env(&env, "openai", "gpt-4");

    // Invalid values should be None, not panic
    assert!(config.temperature.is_none());
    assert!(config.max_tokens.is_none());
    assert!(config.max_tool_turns.is_none()); // Default is None
}

#[test]
fn test_from_env_case_sensitivity() {
    // Test that provider name is uppercased for env var lookup
    let env = env_map(&[("OPENAI_API_KEY", "sk-case-test")]);

    // Should work with lowercase provider name
    let config = Config::from_env(&env, "openai", "gpt-4");
    assert_eq!(config.api_key, Some("sk-case-test".to_string()));

    // Should also work with mixed case
    let config2 = Config::from_env(&env, "OpenAI", "gpt-4");
    assert_eq!(config2.api_key, Some("sk-case-test".to_string()));
}

#[test]
fn test_from_env_max_retries_and_delay() {
    let env = env_map(&[
        ("AGENT_MAX_RETRIES", "7"),
        ("AGENT_RETRY_BASE_DELAY_MS", "500"),
    ]);
    let config = Config::from_env(&env, "openai", "gpt-4");
    assert_eq!(config.max_retries, Some(7u8));
    assert_eq!(config.retry_base_delay_ms, Some(500u64));
}

#[test]
fn test_from_env_output_budget_remedy_mode() {
    let env = env_map(&[("AGENT_OUTPUT_BUDGET_REMEDY_MODE", "shorter_response")]);
    let config = Config::from_env(&env, "openai", "gpt-4");
    assert_eq!(
        config.output_budget_remedy_mode,
        Some("shorter_response".to_string())
    );
}

#[test]
fn test_from_env_output_budget_empty_remedy() {
    let env = env_map(&[("AGENT_OUTPUT_BUDGET_EMPTY_REMEDY", "custom remedy text")]);
    let config = Config::from_env(&env, "openai", "gpt-4");
    assert_eq!(
        config.output_budget_empty_remedy,
        Some("custom remedy text".to_string())
    );
}

#[test]
fn test_from_env_output_budget_raise_enabled() {
    let env = env_map(&[("AGENT_OUTPUT_BUDGET_RAISE_ENABLED", "true")]);
    let config = Config::from_env(&env, "openai", "gpt-4");
    assert_eq!(config.output_budget_raise_enabled, Some(true));
}

#[test]
fn test_from_env_output_budget_raise_multiplier() {
    let env = env_map(&[("AGENT_OUTPUT_BUDGET_RAISE_MULTIPLIER", "3.5")]);
    let config = Config::from_env(&env, "openai", "gpt-4");
    assert_eq!(config.output_budget_raise_multiplier, Some(3.5f64));
}

#[test]
fn test_from_env_output_budget_raise_cap() {
    let env = env_map(&[("AGENT_OUTPUT_BUDGET_RAISE_CAP", "65536")]);
    let config = Config::from_env(&env, "openai", "gpt-4");
    assert_eq!(config.output_budget_raise_cap, Some(65536u32));
}

#[test]
fn test_from_env_read_timeout_secs() {
    let env = env_map(&[("AGENT_READ_TIMEOUT_SECS", "60")]);
    let config = Config::from_env(&env, "openai", "gpt-4");
    assert_eq!(config.read_timeout_secs, Some(60u64));
}

#[test]
fn test_from_env_repetition_guard_false() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("AGENT_REPETITION_GUARD", "false")]);

    // -- Exec
    let config = Config::from_env(&env, "openai", "gpt-4");

    // -- Check
    assert_eq!(config.repetition_guard, Some(false));
    Ok(())
}

#[test]
fn test_from_env_max_tool_turns_defaults_to_none() {
    // Default should be None (no default - runtime decides based on mode)
    let env = EnvMap::new();
    let config = Config::from_env(&env, "openai", "gpt-4");
    assert_eq!(config.max_tool_turns, None);
}
