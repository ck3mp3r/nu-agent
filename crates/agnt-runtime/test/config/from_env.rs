//! `Config::from_env()` tests.

use crate::config::Config;
use crate::test_support::env_map;
use crate::utils::env_map::EnvMap;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

struct ApiKeyCase {
    provider: &'static str,
    model: &'static str,
    vars: &'static [(&'static str, &'static str)],
    expected: Option<&'static str>,
}

#[test]
fn test_from_env_resolves_api_key_from_provider_env() -> Result<()> {
    // -- Setup & Fixtures
    let cases = vec![
        ApiKeyCase {
            provider: "openai",
            model: "gpt-4",
            vars: &[("OPENAI_API_KEY", "sk-test123")],
            expected: Some("sk-test123"),
        },
        ApiKeyCase {
            provider: "nonexistent",
            model: "model-1",
            vars: &[],
            expected: None,
        },
        ApiKeyCase {
            provider: "OpenAI",
            model: "gpt-4",
            vars: &[("OPENAI_API_KEY", "sk-case-test")],
            expected: Some("sk-case-test"),
        },
    ];

    // -- Exec & Check
    for case in cases {
        let env = env_map(case.vars);
        let config = Config::from_env(&env, case.provider, case.model);
        assert_eq!(config.provider, case.provider);
        assert_eq!(config.model, case.model);
        assert_eq!(
            config.api_key.as_deref(),
            case.expected,
            "provider {}",
            case.provider
        );
        assert!(config.max_tool_turns.is_none(), "default is None");
    }
    Ok(())
}

#[test]
fn test_from_env_applies_agent_overrides() -> Result<()> {
    // -- Setup & Fixtures
    let full = env_map(&[
        ("ANTHROPIC_API_KEY", "sk-ant-test"),
        ("AGENT_TEMPERATURE", "0.8"),
        ("AGENT_MAX_TOKENS", "2000"),
        ("AGENT_MAX_CONTEXT_TOKENS", "8192"),
        ("AGENT_MAX_OUTPUT_TOKENS", "4096"),
        ("AGENT_MAX_TOOL_TURNS", "15"),
        ("AGENT_BASE_URL", "https://custom.api.com"),
        ("AGENT_MAX_TOOL_RESULT_BYTES", "15000"),
    ]);

    // -- Exec
    let config = Config::from_env(&full, "anthropic", "claude-3-opus");

    // -- Check
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

    // -- Check: partial overrides leave the rest unset
    let partial = env_map(&[
        ("OPENAI_API_KEY", "sk-partial"),
        ("AGENT_TEMPERATURE", "0.5"),
    ]);
    let config = Config::from_env(&partial, "openai", "gpt-3.5-turbo");
    assert_eq!(config.temperature, Some(0.5));
    assert!(config.max_tokens.is_none());
    assert!(config.base_url.is_none());
    assert!(config.max_tool_turns.is_none());
    Ok(())
}

#[test]
fn test_from_env_ignores_invalid_numeric_values() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[
        ("AGENT_TEMPERATURE", "not-a-number"),
        ("AGENT_MAX_TOKENS", "invalid"),
        ("AGENT_MAX_TOOL_TURNS", "-5"),
    ]);

    // -- Exec
    let config = Config::from_env(&env, "openai", "gpt-4");

    // -- Check
    assert!(config.temperature.is_none());
    assert!(config.max_tokens.is_none());
    assert!(config.max_tool_turns.is_none());
    Ok(())
}

#[test]
fn test_from_env_reads_remaining_agent_vars() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[
        ("AGENT_MAX_RETRIES", "7"),
        ("AGENT_RETRY_BASE_DELAY_MS", "500"),
        ("AGENT_OUTPUT_BUDGET_REMEDY_MODE", "shorter_response"),
        ("AGENT_OUTPUT_BUDGET_EMPTY_REMEDY", "custom remedy text"),
        ("AGENT_OUTPUT_BUDGET_RAISE_ENABLED", "true"),
        ("AGENT_OUTPUT_BUDGET_RAISE_MULTIPLIER", "3.5"),
        ("AGENT_OUTPUT_BUDGET_RAISE_CAP", "65536"),
        ("AGENT_READ_TIMEOUT_SECS", "60"),
        ("AGENT_REPETITION_GUARD", "false"),
    ]);

    // -- Exec
    let config = Config::from_env(&env, "openai", "gpt-4");

    // -- Check
    assert_eq!(config.max_retries, Some(7u8));
    assert_eq!(config.retry_base_delay_ms, Some(500u64));
    assert_eq!(
        config.output_budget_remedy_mode,
        Some("shorter_response".to_string())
    );
    assert_eq!(
        config.output_budget_empty_remedy,
        Some("custom remedy text".to_string())
    );
    assert_eq!(config.output_budget_raise_enabled, Some(true));
    assert_eq!(config.output_budget_raise_multiplier, Some(3.5f64));
    assert_eq!(config.output_budget_raise_cap, Some(65536u32));
    assert_eq!(config.read_timeout_secs, Some(60u64));
    assert_eq!(config.repetition_guard, Some(false));

    // -- Check: unset vars stay None
    let empty = Config::from_env(&EnvMap::new(), "openai", "gpt-4");
    assert_eq!(empty.max_tool_turns, None);
    Ok(())
}
