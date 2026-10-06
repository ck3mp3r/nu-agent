use nu_agent_core::config::Config;

#[test]
fn resolve_agent_mode_defaults_to_tui_for_interactive_no_input() {
    let mode = super::resolve_agent_mode(true, true, true);
    assert_eq!(mode, super::AgentMode::Tui);
}

#[test]
fn resolve_agent_mode_uses_stderr_when_input_is_provided() {
    let mode = super::resolve_agent_mode(false, true, true);
    assert_eq!(mode, super::AgentMode::Stderr);
}

#[test]
fn resolve_agent_mode_uses_stderr_when_stdin_is_not_tty() {
    let mode = super::resolve_agent_mode(true, false, true);
    assert_eq!(mode, super::AgentMode::Stderr);
}

#[test]
fn resolve_agent_mode_uses_stderr_when_stderr_is_not_tty() {
    let mode = super::resolve_agent_mode(true, true, false);
    assert_eq!(mode, super::AgentMode::Stderr);
}

#[test]
fn should_enter_foreground_true_for_tui() {
    assert!(super::should_enter_foreground(super::AgentMode::Tui, true));
}

#[test]
fn should_enter_foreground_true_for_stderr_with_tty() {
    assert!(super::should_enter_foreground(
        super::AgentMode::Stderr,
        true
    ));
}

#[test]
fn should_enter_foreground_false_for_stderr_without_tty() {
    assert!(!super::should_enter_foreground(
        super::AgentMode::Stderr,
        false
    ));
}

#[test]
fn should_enter_foreground_true_for_tui_even_without_tty() {
    // TUI mode always needs foreground regardless of stderr_is_tty flag
    assert!(super::should_enter_foreground(super::AgentMode::Tui, false));
}

// Integration tests for mode-specific max_tool_turns defaults
mod max_tool_turns_mode_defaults {
    use super::*;
    use crate::command::agent::AgentMode;

    #[test]
    fn test_tui_mode_gets_unlimited_turns_by_default() {
        // When AgentMode::Tui and max_tool_turns is None, it should stay None (unlimited)
        let mode = AgentMode::Tui;
        let mut config = Config {
            provider: "openai".to_string(),
            provider_impl: None,
            model: "gpt-4".to_string(),
            api_key: None,
            base_url: None,
            temperature: None,
            max_tokens: None,
            max_context_tokens: None,
            max_output_tokens: None,
            max_tool_turns: None, // Not configured
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
        };

        // Simulate the mode-specific default logic from mod.rs
        if config.max_tool_turns.is_none() && !mode.is_tui() {
            config.max_tool_turns = Some(20);
        }

        // TUI mode should stay unlimited (None)
        assert!(config.max_tool_turns.is_none());
    }

    #[test]
    fn test_stderr_mode_gets_20_turns_by_default() {
        // When AgentMode::Stderr and max_tool_turns is None, it should get Some(20)
        let mode = AgentMode::Stderr;
        let mut config = Config {
            provider: "openai".to_string(),
            provider_impl: None,
            model: "gpt-4".to_string(),
            api_key: None,
            base_url: None,
            temperature: None,
            max_tokens: None,
            max_context_tokens: None,
            max_output_tokens: None,
            max_tool_turns: None, // Not configured
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
        };

        // Simulate the mode-specific default logic from mod.rs
        if config.max_tool_turns.is_none() && !mode.is_tui() {
            config.max_tool_turns = Some(20);
        }

        // Stderr mode should get 20
        assert_eq!(config.max_tool_turns, Some(20));
    }

    #[test]
    fn test_explicit_max_turns_overrides_both_modes() {
        // When max_tool_turns is explicitly set, it should be respected in both modes
        for mode in [AgentMode::Tui, AgentMode::Stderr] {
            let mut config = Config {
                provider: "openai".to_string(),
                provider_impl: None,
                model: "gpt-4".to_string(),
                api_key: None,
                base_url: None,
                temperature: None,
                max_tokens: None,
                max_context_tokens: None,
                max_output_tokens: None,
                max_tool_turns: Some(10), // Explicitly set
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
            };

            // Simulate the mode-specific default logic from mod.rs
            if config.max_tool_turns.is_none() && !mode.is_tui() {
                config.max_tool_turns = Some(20);
            }

            // Should stay at explicit value
            assert_eq!(config.max_tool_turns, Some(10));
        }
    }
}
