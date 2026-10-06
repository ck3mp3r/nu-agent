//! `Config::validate()` tests.

use crate::config::Config;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_validate_output_budget_raise_multiplier_invalid() -> Result<()> {
    let config = Config {
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        output_budget_raise_multiplier: Some(1.0),
        ..Config::default()
    };
    let err = match config.validate() {
        Ok(_) => return Err("multiplier <= 1.0 should fail validation".into()),
        Err(e) => e,
    };
    assert!(err.contains("output_budget_raise_multiplier"));
    Ok(())
}

#[test]
fn test_validate_output_budget_raise_cap_invalid() -> Result<()> {
    let config = Config {
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        output_budget_raise_cap: Some(0),
        ..Config::default()
    };
    let err = match config.validate() {
        Ok(_) => return Err("cap 0 should fail validation".into()),
        Err(e) => e,
    };
    assert!(err.contains("output_budget_raise_cap"));
    Ok(())
}

#[test]
fn test_validate_output_budget_remedy_mode_invalid() -> Result<()> {
    let config = Config {
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        output_budget_remedy_mode: Some("bogus".to_string()),
        ..Config::default()
    };
    let err = match config.validate() {
        Ok(_) => return Err("invalid remedy mode should fail validation".into()),
        Err(e) => e,
    };
    assert!(err.contains("output_budget_remedy_mode"));
    Ok(())
}

#[test]
fn test_validate_output_budget_remedy_mode_valid() -> Result<()> {
    for mode in ["empty_output", "shorter_response"] {
        let config = Config {
            provider: "openai".to_string(),
            model: "gpt-4".to_string(),
            output_budget_remedy_mode: Some(mode.to_string()),
            ..Config::default()
        };
        assert!(config.validate().is_ok(), "mode {mode} must validate");
    }
    Ok(())
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
        output_budget_empty_remedy: None,
        output_budget_remedy_mode: None,
        output_budget_raise_enabled: None,
        output_budget_raise_multiplier: None,
        output_budget_raise_cap: None,
        repetition_guard: None,
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
        output_budget_empty_remedy: None,
        output_budget_remedy_mode: None,
        output_budget_raise_enabled: None,
        output_budget_raise_multiplier: None,
        output_budget_raise_cap: None,
        repetition_guard: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_empty_provider() -> Result<()> {
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
        output_budget_empty_remedy: None,
        output_budget_remedy_mode: None,
        output_budget_raise_enabled: None,
        output_budget_raise_multiplier: None,
        output_budget_raise_cap: None,
        repetition_guard: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    let err = match config.validate() {
        Ok(_) => return Err("empty provider should fail validation".into()),
        Err(e) => e,
    };
    assert!(err.contains("provider"));
    Ok(())
}

#[test]
fn test_validate_empty_model() -> Result<()> {
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
        output_budget_empty_remedy: None,
        output_budget_remedy_mode: None,
        output_budget_raise_enabled: None,
        output_budget_raise_multiplier: None,
        output_budget_raise_cap: None,
        repetition_guard: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    let err = match config.validate() {
        Ok(_) => return Err("empty model should fail validation".into()),
        Err(e) => e,
    };
    assert!(err.contains("model"));
    Ok(())
}

#[test]
fn test_validate_max_output_exceeds_context() -> Result<()> {
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
        output_budget_empty_remedy: None,
        output_budget_remedy_mode: None,
        output_budget_raise_enabled: None,
        output_budget_raise_multiplier: None,
        output_budget_raise_cap: None,
        repetition_guard: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    let err = match config.validate() {
        Ok(_) => return Err("max output exceeding context should fail validation".into()),
        Err(e) => e,
    };
    assert!(err.contains("max_output_tokens"));
    assert!(err.contains("max_context_tokens"));
    Ok(())
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
        output_budget_empty_remedy: None,
        output_budget_remedy_mode: None,
        output_budget_raise_enabled: None,
        output_budget_raise_multiplier: None,
        output_budget_raise_cap: None,
        repetition_guard: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_zero_max_tool_turns() -> Result<()> {
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
        output_budget_empty_remedy: None,
        output_budget_remedy_mode: None,
        output_budget_raise_enabled: None,
        output_budget_raise_multiplier: None,
        output_budget_raise_cap: None,
        repetition_guard: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    let result = config.validate();
    let err = match result {
        Ok(_) => return Err("zero max_tool_turns should fail validation".into()),
        Err(e) => e,
    };
    assert!(err.contains("max_tool_turns"));
    Ok(())
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
        output_budget_empty_remedy: None,
        output_budget_remedy_mode: None,
        output_budget_raise_enabled: None,
        output_budget_raise_multiplier: None,
        output_budget_raise_cap: None,
        repetition_guard: None,
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
        output_budget_empty_remedy: None,
        output_budget_remedy_mode: None,
        output_budget_raise_enabled: None,
        output_budget_raise_multiplier: None,
        output_budget_raise_cap: None,
        repetition_guard: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_context_warning_threshold_zero_is_err() -> Result<()> {
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        context_warning_threshold: Some(0.0),
        ..Config::default()
    };
    let result = config.validate();
    let err = match result {
        Ok(_) => return Err("zero context_warning_threshold should fail validation".into()),
        Err(e) => e,
    };
    assert!(err.contains("context_warning_threshold"));
    Ok(())
}

#[test]
fn test_validate_context_warning_threshold_above_one_is_err() -> Result<()> {
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        context_warning_threshold: Some(1.1),
        ..Config::default()
    };
    let result = config.validate();
    let err = match result {
        Ok(_) => return Err("above-one context_warning_threshold should fail validation".into()),
        Err(e) => e,
    };
    assert!(err.contains("context_warning_threshold"));
    Ok(())
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
fn test_validate_model_context_tokens_zero_is_err() -> Result<()> {
    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        model_context_tokens: Some(0),
        ..Config::default()
    };
    let result = config.validate();
    let err = match result {
        Ok(_) => return Err("zero model_context_tokens should fail validation".into()),
        Err(e) => e,
    };
    assert!(err.contains("model_context_tokens"));
    Ok(())
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
        output_budget_empty_remedy: None,
        output_budget_remedy_mode: None,
        output_budget_raise_enabled: None,
        output_budget_raise_multiplier: None,
        output_budget_raise_cap: None,
        repetition_guard: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_zero_max_tool_turns_still_invalid() -> Result<()> {
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
        output_budget_empty_remedy: None,
        output_budget_remedy_mode: None,
        output_budget_raise_enabled: None,
        output_budget_raise_multiplier: None,
        output_budget_raise_cap: None,
        repetition_guard: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    let result = config.validate();
    let err = match result {
        Ok(_) => return Err("zero max_tool_turns should still be invalid".into()),
        Err(e) => e,
    };
    assert!(err.contains("max_tool_turns"));
    Ok(())
}
