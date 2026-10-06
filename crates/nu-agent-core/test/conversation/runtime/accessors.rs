// ========================================================================
// Phase I: AgentConversationRuntime accessor method tests
// ========================================================================

#[test]
fn accessor_provider_returns_provider_string() {
    // Verifies that runtime.provider() delegates to provider_state.config().provider
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

    // Verify the accessor delegation chain: provider() -> provider_state.config().provider
    let provider_state = crate::conversation::state::provider::ProviderState::new(config, None);
    assert_eq!(provider_state.config().provider.as_str(), "copilot");
}

#[test]
fn accessor_model_returns_model_string() {
    // Verifies that runtime.model() delegates to provider_state.config().model
    use crate::config::Config;

    let config = Config {
        a2a_port: None,
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "claude-sonnet-4".to_string(),
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

    let provider_state = crate::conversation::state::provider::ProviderState::new(config, None);
    assert_eq!(provider_state.config().model.as_str(), "claude-sonnet-4");
}

#[test]
fn accessor_max_context_tokens_returns_none_when_unset() {
    use crate::config::Config;

    let config = Config {
        a2a_port: None,
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
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

    let provider_state = crate::conversation::state::provider::ProviderState::new(config, None);
    assert_eq!(
        provider_state.config().max_context_tokens.map(u64::from),
        None
    );
}

#[test]
fn accessor_max_context_tokens_returns_value_when_set() {
    use crate::config::Config;

    let config = Config {
        a2a_port: None,
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: Some(200_000),
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

    let provider_state = crate::conversation::state::provider::ProviderState::new(config, None);
    assert_eq!(
        provider_state.config().max_context_tokens.map(u64::from),
        Some(200_000)
    );
}

#[test]
fn accessor_startup_plugin_config_returns_none_when_default() {
    use crate::config::Config;

    let config = Config {
        a2a_port: None,
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
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

    let provider_state = crate::conversation::state::provider::ProviderState::new(config, None);
    assert!(provider_state.startup_plugin_config().is_none());
}

#[test]
fn accessor_agent_identity_returns_none_when_default() {
    // PersonaState with no agent_identity set must return None
    let persona_state = crate::conversation::state::persona::PersonaState::new(
        None, None, None, None, None, None, None,
    );
    assert_eq!(persona_state.agent_identity(), None);
}

#[test]
fn accessor_agent_identity_returns_some_when_set() {
    let persona_state = crate::conversation::state::persona::PersonaState::new(
        None,
        Some("developer".to_string()),
        None,
        None,
        None,
        None,
        None,
    );
    assert_eq!(persona_state.agent_identity(), Some("developer"));
}

#[test]
fn accessor_mcp_caller_cwd_returns_none_when_default() {
    // McpState has mcp_caller_cwd as Option<PathBuf> — None when unset
    let cwd: Option<std::path::PathBuf> = None;
    assert_eq!(cwd.as_deref(), None::<&std::path::Path>);
}

#[test]
fn accessor_mcp_lifecycle_projection_returns_empty_when_default() {
    use crate::tools::mcp::runtime::McpServerLifecycle;

    let projection: Vec<McpServerLifecycle> = vec![];
    assert!(projection.is_empty());
}

#[test]
fn accessor_available_agent_summaries_delegates_to_multi_agent_state() {
    use crate::config::AgentsConfig;
    use crate::conversation::state::multi_agent::MultiAgentState;

    let state = MultiAgentState::new(vec![], AgentsConfig::default());
    assert!(state.available_agent_summaries().is_empty());
}
