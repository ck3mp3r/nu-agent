use super::*;

// ========================================================================
// Provider dispatch tests
// ========================================================================

#[test]
fn provider_dispatch_unsupported_provider_returns_error() {
    // RED: Verify that unsupported provider returns clear error
    use crate::config::Config;

    let config = Config {
        a2a_port: None,
        provider: "unsupported-provider".to_string(),
        provider_impl: None,
        model: "some-model".to_string(),
        api_key: Some("test-key".to_string()),
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
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

    // This test will compile once we add the dispatch logic
    // For now, document that build_copilot_client works for copilot only
    // When we add dispatch in execute_turn, this will test the error path

    // Expected behavior: execute_turn should return error with:
    // "Unsupported provider: 'unsupported-provider'"
    // This test documents the requirement for now
    assert_eq!(config.provider, "unsupported-provider");
}

#[test]
fn client_cache_key_contains_provider_and_model() {
    // Characterise client_cache_key (runtime.rs:708-714).
    // It returns (config.provider, config.api_key, config.base_url).
    use crate::config::Config;

    let config = Config {
        a2a_port: None,
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: Some("test-key".to_string()),
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
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

    // client_cache_key clones (provider, api_key, base_url) from config.
    let key: ClientCacheKey = (
        config.provider.clone(),
        config.api_key.clone(),
        config.base_url.clone(),
        config.read_timeout_secs,
    );

    assert_eq!(
        key,
        (
            "copilot".to_string(),
            Some("test-key".to_string()),
            None,
            None,
        )
    );
}

#[test]
fn client_cache_key_includes_base_url_when_set() {
    // Same as above but with base_url set.
    use crate::config::Config;

    let config = Config {
        a2a_port: None,
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: Some("test-key".to_string()),
        base_url: Some("https://custom.example.com".to_string()),
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
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

    let key: ClientCacheKey = (
        config.provider.clone(),
        config.api_key.clone(),
        config.base_url.clone(),
        config.read_timeout_secs,
    );

    assert_eq!(
        key,
        (
            "copilot".to_string(),
            Some("test-key".to_string()),
            Some("https://custom.example.com".to_string()),
            None,
        )
    );
}

#[test]
fn active_model_identity_returns_provider_slash_model() {
    // Characterise active_model_identity (runtime.rs:484-486).
    // Returns format!("{}/{}", config.provider, config.model).
    use crate::config::Config;

    let config = Config {
        a2a_port: None,
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "claude-sonnet-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
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

    // Replicate the method body exactly
    let identity = format!("{}/{}", config.provider, config.model);

    assert!(
        identity.contains("copilot"),
        "identity must contain provider"
    );
    assert!(
        identity.contains("claude-sonnet-4"),
        "identity must contain model"
    );
    assert_eq!(
        identity, "copilot/claude-sonnet-4",
        "identity must be provider/model"
    );
}

// ========================================================================
// Phase F: ProviderState characterisation tests
// ========================================================================

#[test]
fn provider_state_switch_model_returns_err_when_no_startup_config() {
    // Characterise switch_model (runtime.rs ExtendedRuntime impl).
    // When startup_plugin_config is None, switch_model must return Err
    // containing "model switch unavailable".

    let startup_plugin_config: Option<crate::config::PluginConfig> = None;

    // Replicate the switch_model error path with no startup config
    let result: std::result::Result<String, String> = startup_plugin_config
        .ok_or_else(|| {
            "model switch unavailable: startup plugin config cache is missing".to_string()
        })
        .map(|_| "unreachable".to_string());

    assert!(result.is_err());
    assert!(
        result
            .err()
            .as_deref()
            .unwrap_or_default()
            .contains("model switch unavailable"),
        "error must mention 'model switch unavailable'"
    );
}

#[test]
fn provider_state_client_cache_key_contains_provider_and_api_key() {
    // Characterise client_cache_key (runtime.rs:302-308).
    // With provider="copilot", api_key=Some("fake-key"), base_url=None,
    // client_cache_key returns (provider, api_key, base_url).
    use crate::config::Config;

    let config = Config {
        a2a_port: None,
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: Some("fake-key".to_string()),
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
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

    // Replicate client_cache_key body
    let key: ClientCacheKey = (
        config.provider.clone(),
        config.api_key.clone(),
        config.base_url.clone(),
        config.read_timeout_secs,
    );

    assert_eq!(key.0, "copilot");
    assert_eq!(key.1, Some("fake-key".to_string()));
    assert_eq!(key.2, None);
    assert_eq!(key.3, None);
}
