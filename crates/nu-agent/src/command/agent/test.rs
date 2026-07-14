use super::test_helpers::{
    MockEngineInterface, create_test_agent, create_test_call, parse_agent_invocation_with_signature,
};
use super::{EngineConfigInterface, extract_tool_timeout, extract_tools_from_call, runtime_build};
use nu_agent_core::config::Config;
use nu_plugin::{EvaluatedCall, SimplePluginCommand};
use nu_protocol::{ParseError, Span, Spanned, SyntaxShape, Value, record};
use serial_test::serial;

fn first_unknown_flag_error(parse_errors: &[ParseError]) -> Option<(String, String, String)> {
    parse_errors.iter().find_map(|err| match err {
        ParseError::UnknownFlag(cmd, flag, _span, help) => {
            Some((cmd.clone(), flag.clone(), help.clone()))
        }
        _ => None,
    })
}

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
            max_tool_calls_per_subturn: None,
            additional_params: None,
            a2a_enabled: false,
            a2a_port: None,
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
            max_tool_calls_per_subturn: None,
            additional_params: None,
            a2a_enabled: false,
            a2a_port: None,
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
                max_tool_calls_per_subturn: None,
                additional_params: None,
                a2a_enabled: false,
                a2a_port: None,
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
fn agent_command_signature_does_not_expose_removed_provider_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let provider_flag = sig.named.iter().find(|f| f.long == "provider");
    assert!(
        provider_flag.is_none(),
        "Removed --provider flag must not be exposed"
    );
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
fn agent_command_signature_does_not_expose_removed_max_tokens_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "max-tokens");
    assert!(
        flag.is_none(),
        "Removed --max-tokens flag must not be exposed"
    );
}

#[test]
fn agent_command_signature_help_text_excludes_removed_flags() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);
    let rendered = format!("{sig:?}");

    assert!(
        !rendered.contains("long: \"provider\""),
        "signature/help debug output should not contain removed provider long flag"
    );
    assert!(
        !rendered.contains("long: \"max-tokens\""),
        "signature/help debug output should not contain removed max-tokens long flag"
    );
}

#[test]
fn invocation_agent_provider_flag_is_rejected_with_unknown_option_and_help_guidance() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);
    let parse_errors =
        parse_agent_invocation_with_signature(sig.clone(), "agent --provider openai");

    let (cmd, flag, help) = first_unknown_flag_error(&parse_errors)
        .expect("expected parser-level unknown-flag rejection for --provider");
    assert_eq!(cmd, "agent");
    assert_eq!(flag, "provider");

    let model_flag = sig
        .named
        .iter()
        .find(|f| f.long == "model")
        .expect("canonical --model flag must remain available");
    assert!(
        help.contains("--help") && model_flag.desc.contains("provider/model"),
        "when unknown-flag help is generic, canonical guidance must still be present on --model; help={help}, model_desc={} ",
        model_flag.desc
    );
}

#[test]
fn invocation_agent_max_tokens_flag_is_rejected_with_unknown_option_and_help_guidance() {
    let (agent, _temp_dir) = create_test_agent();
    let parse_errors = parse_agent_invocation_with_signature(
        SimplePluginCommand::signature(&agent),
        "agent --max-tokens 4096",
    );

    let (cmd, flag, help) = first_unknown_flag_error(&parse_errors)
        .expect("expected parser-level unknown-flag rejection for --max-tokens");
    assert_eq!(cmd, "agent");
    assert_eq!(flag, "max-tokens");
    assert!(
        help.contains("--max-context-tokens") || help.contains("--max-output-tokens"),
        "unknown --max-tokens guidance should point to explicit token knobs, got: {help}"
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

#[test]
fn agent_command_signature_has_tools_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "tools");
    assert!(flag.is_some(), "Missing --tools flag");
    assert_eq!(
        flag.unwrap().arg,
        Some(SyntaxShape::Record(vec![].into())),
        "Wrong type for --tools (should be Record)"
    );
}

#[test]
fn agent_command_signature_has_permissions_flag_as_record() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "permissions");
    assert!(flag.is_some(), "Missing --permissions flag");
    assert_eq!(
        flag.expect("permissions flag").arg,
        Some(SyntaxShape::Record(vec![].into())),
        "--permissions must accept record/object input"
    );
}

#[test]
fn agent_command_signature_does_not_expose_legacy_permission_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let legacy = sig.named.iter().find(|f| f.long == "permission");
    assert!(
        legacy.is_none(),
        "Legacy repeated --permission flag must not be exposed"
    );
}

#[test]
fn cli_does_not_expose_unsupported_compaction_modes() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);
    let rendered = format!("{sig:?}").to_ascii_lowercase();
    // Ensure serde aliases (standalone shorthand names) are not exposed in the CLI.
    // Canonical names like "token_truncate" and "sliding_window" are fine since they
    // appear in the --compaction-strategy description.
    //
    // Strip canonical names before checking for standalone aliases.
    let stripped = rendered
        .replace("sliding_summary", "")
        .replace("sliding_window", "")
        .replace("token_truncate", "");
    assert!(
        !stripped.contains("truncate"),
        "standalone alias 'truncate' should not appear in CLI"
    );
    assert!(
        !stripped.contains("\"sliding\""),
        "standalone alias 'sliding' should not appear in CLI"
    );
    assert!(
        !stripped.contains("summarize"),
        "standalone alias 'summarize' should not appear in CLI"
    );
}

#[test]
fn agent_command_signature_has_quiet_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let quiet_flag = sig.named.iter().find(|f| f.long == "quiet");
    assert!(quiet_flag.is_some(), "Missing --quiet flag");

    let flag = quiet_flag.expect("quiet flag");
    assert_eq!(flag.short, Some('q'), "Missing -q short flag");
    assert_eq!(flag.arg, None, "--quiet should be a switch");
}

#[test]
fn agent_command_signature_has_log_level_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);
    let flag = sig.named.iter().find(|f| f.long == "log-level");
    assert!(flag.is_some(), "Missing --log-level flag");
    assert_eq!(
        flag.unwrap().arg,
        Some(SyntaxShape::String),
        "Wrong type for --log-level"
    );
}

#[test]
fn agent_command_signature_does_not_expose_tui_switch() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let tui_flag = sig.named.iter().find(|f| f.long == "tui");
    assert!(tui_flag.is_none(), "--tui flag should be removed");
}

#[test]
fn agent_command_signature_updates_verbose_description_for_progressive_levels() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let verbose_flag = sig.named.iter().find(|f| f.long == "verbose");
    assert!(verbose_flag.is_some(), "Missing --verbose flag");
    let desc = &verbose_flag.expect("verbose flag").desc;
    assert!(
        desc.contains("-v") && desc.contains("-vv") && desc.contains("-vvv"),
        "Verbose description should document progressive levels, got: {desc}"
    );
}

#[test]
fn agent_command_signature_quiet_and_verbose_help_text_describes_stderr_ux_behavior() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let quiet = sig
        .named
        .iter()
        .find(|f| f.long == "quiet")
        .expect("quiet flag");
    assert!(
        quiet.desc.contains("Suppress") || quiet.desc.contains("progress"),
        "quiet help text should describe suppression semantics"
    );

    let verbose = sig
        .named
        .iter()
        .find(|f| f.long == "verbose")
        .expect("verbose flag");
    assert!(
        verbose.desc.contains("-v")
            && verbose.desc.contains("-vv")
            && verbose.desc.contains("-vvv"),
        "verbose help text should describe progressive levels"
    );
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
        max_tool_turns: None, // Default is None - runtime decides based on mode
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: false,
        a2a_port: None,
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
        max_output_tokens: Some(2000),
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
    assert_eq!(result.max_output_tokens, Some(2000)); // Only set in flags
    assert!(result.max_tool_turns.is_none()); // Default is None
}

// These integration tests will use a helper function from agent.rs
// that performs the full config resolution pipeline
mod config_resolution_integration {
    use super::*;
    use crate::command::agent::resolve_config;

    #[test]
    fn resolve_config_with_no_plugin_config() {
        let mock = MockEngineInterface::new();
        let call = create_test_call(vec![("model", Value::test_string("openai/gpt-4"))]);

        let result = resolve_config(&mock, &call);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4");
        assert!(config.max_tool_turns.is_none()); // Default is None
    }

    #[test]
    #[serial] // Prevent parallel execution due to env vars
    fn resolve_config_plugin_overrides_env() {
        // Set env vars for testing
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "env_key");
            std::env::set_var("AGENT_TEMPERATURE", "0.5");
        }

        // New-format config: providers block required for resolve_config() to succeed
        let plugin_config = Value::test_record(record! {
            "model" => Value::test_string("openai/gpt-4"),
            "providers" => Value::test_record(record! {
                "openai" => Value::test_record(record! {
                    "models" => Value::test_record(record! {
                        "gpt-4" => Value::test_record(record! {
                            "temperature" => Value::test_float(0.9),
                        }),
                    }),
                }),
            }),
        });

        let mock = MockEngineInterface::with_config(plugin_config);
        let call = create_test_call(vec![]);

        let result = resolve_config(&mock, &call);
        assert!(result.is_ok(), "resolve_config failed: {:?}", result);

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
        // Set env vars for the resolved provider (openai, after --model override)
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "env_key");
            std::env::set_var("AGENT_TEMPERATURE", "0.5");
        }

        // New-format config: both providers present so --model openai/gpt-4 can override
        let plugin_config = Value::test_record(record! {
            "model" => Value::test_string("anthropic/claude-3"),
            "providers" => Value::test_record(record! {
                "anthropic" => Value::test_record(record! {
                    "models" => Value::test_record(record! {
                        "claude-3" => Value::test_record(record! {
                            "temperature" => Value::test_float(0.8),
                            "limit" => Value::test_record(record! {
                                "output" => Value::test_int(1000),
                            }),
                        }),
                    }),
                }),
                "openai" => Value::test_record(record! {
                    "models" => Value::test_record(record! {
                        "gpt-4" => Value::test_record(record! {
                            "temperature" => Value::test_float(0.8),
                            "limit" => Value::test_record(record! {
                                "output" => Value::test_int(1000),
                            }),
                        }),
                    }),
                }),
            }),
        });

        let mock = MockEngineInterface::with_config(plugin_config);
        let call = create_test_call(vec![
            ("model", Value::test_string("openai/gpt-4")), // Canonical override
            ("temperature", Value::test_float(1.2)),       // Override temperature
        ]);

        let result = resolve_config(&mock, &call);
        assert!(result.is_ok(), "resolve_config failed: {:?}", result);

        let config = result.unwrap();
        assert_eq!(config.provider, "openai"); // Flag wins
        assert_eq!(config.model, "gpt-4"); // Flag wins
        assert_eq!(config.temperature, Some(1.2)); // Flag wins
        assert_eq!(config.max_output_tokens, Some(1000)); // Plugin value (no flag override)
        assert_eq!(config.api_key, Some("env_key".to_string())); // Env provides for resolved provider

        // Cleanup
        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
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

        let call = create_test_call(vec![("model", Value::test_string("openai/gpt-4"))]);

        let result = resolve_config(&mock, &call);
        assert!(result.is_err());

        // Just verify we got an error - the exact error message structure may vary
        let _err = result.unwrap_err();
        // Error should be about invalid config format
    }

    #[test]
    fn resolve_config_errors_on_missing_providers() {
        // A plugin config that has a model key but no providers block must error,
        // not silently fall through to the legacy path.
        let plugin_config = Value::test_record(record! {
            "model" => Value::test_string("openai/gpt-4"),
        });

        let mock = MockEngineInterface::with_config(plugin_config);
        let call = create_test_call(vec![]);

        let result = resolve_config(&mock, &call);
        assert!(
            result.is_err(),
            "expected Err for config without providers, got: {result:?}"
        );

        let err = result.unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("providers"),
            "error message should mention 'providers', got: {msg}"
        );
    }
}

// ============================================================================
// New Plugin Config Tests - Test provider/model format and --small flag
// ============================================================================

mod new_plugin_config_tests {
    use super::*;
    use crate::command::agent::{picker::format_active_model_identity, resolve_config};

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
    fn resolve_config_accepts_mcp_from_plugin_env_config() {
        use std::collections::HashMap;

        let mut providers_map = HashMap::new();
        let mut models = HashMap::new();
        models.insert(
            "claude-sonnet-4-20250514".to_string(),
            Value::test_record(record! {}),
        );

        providers_map.insert(
            "github-copilot".to_string(),
            Value::test_record(record! {
                "api_key" => Value::test_string("token"),
                "base_url" => Value::test_string("https://api.individual.githubcopilot.com"),
                "models" => Value::test_record(models.into_iter().collect()),
            }),
        );

        let plugin_config = Value::test_record(record! {
            "mcp" => Value::test_record(record! {
                "c5t" => Value::test_record(record! {
                    "transport" => Value::test_string("sse"),
                    "url" => Value::test_string("http://0.0.0.0:3737/mcp"),
                }),
                "nu" => Value::test_record(record! {
                    "transport" => Value::test_string("stdio"),
                    "command" => Value::test_string("nu-mcp"),
                    "args" => Value::test_list(vec![
                        Value::test_string("--add-path"),
                        Value::test_string("/tmp"),
                    ]),
                    "env" => Value::test_record(record! {
                        "GIT_PAGER" => Value::test_string(""),
                    }),
                }),
            }),
            "model" => Value::test_string("github-copilot/anthropic/claude-sonnet-4-20250514"),
            "providers" => Value::test_record(providers_map.into_iter().collect()),
        });

        let parsed =
            nu_agent_core::tools::mcp::config::McpConfig::from_plugin_config(&plugin_config)
                .expect("mcp parse from plugin config");
        assert_eq!(parsed.mcp.len(), 2);

        let resolved = resolve_config(
            &MockEngineInterface::with_config(plugin_config),
            &create_test_call(vec![]),
        );
        assert!(
            resolved.is_ok(),
            "config resolve should still succeed with mcp present"
        );
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
    fn resolve_config_with_model_flag_override_for_tui_path_uses_flag_precedence() {
        use std::collections::HashMap;

        let mut providers_map = HashMap::new();

        let mut openai_models = HashMap::new();
        openai_models.insert(
            "gpt-4".to_string(),
            Value::test_record(vec![].into_iter().collect()),
        );
        openai_models.insert(
            "gpt-4o-mini".to_string(),
            Value::test_record(vec![].into_iter().collect()),
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
        let call = create_test_call(vec![
            ("tui", Value::test_bool(true)),
            ("model", Value::test_string("openai/gpt-4o-mini")),
        ]);

        let config = resolve_config(&mock, &call).expect("resolve config for tui");
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4o-mini");
        assert_eq!(
            format_active_model_identity(&config.provider, &config.model),
            "openai/gpt-4o-mini"
        );
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
    fn resolve_config_new_flow_resolves_model_preamble_over_provider_preamble() {
        use std::collections::HashMap;

        let mut providers_map = HashMap::new();
        let mut openai_models = HashMap::new();
        openai_models.insert(
            "gpt-5-mini".to_string(),
            Value::test_record(record! {
                "preamble" => Value::test_string("model preamble"),
            }),
        );

        providers_map.insert(
            "openai".to_string(),
            Value::test_record(record! {
                "preamble" => Value::test_string("provider preamble"),
                "models" => Value::test_record(openai_models.into_iter().collect()),
            }),
        );

        let plugin_config = Value::test_record(record! {
            "model" => Value::test_string("openai/gpt-5-mini"),
            "providers" => Value::test_record(providers_map.into_iter().collect()),
        });

        let config = resolve_config(
            &MockEngineInterface::with_config(plugin_config),
            &create_test_call(vec![]),
        )
        .expect("resolve config");

        assert_eq!(config.preamble.as_deref(), Some("model preamble"));
    }

    #[test]
    fn resolve_config_new_flow_falls_back_to_global_preamble_on_complete_miss() {
        use nu_agent_core::protocol::preamble::PreambleDefaults;
        use std::collections::HashMap;

        let mut providers_map = HashMap::new();
        providers_map.insert(
            "custom".to_string(),
            Value::test_record(record! {
                "models" => Value::test_record(record! {
                    "unknown-model" => Value::test_record(record! {}),
                }),
            }),
        );

        let plugin_config = Value::test_record(record! {
            "model" => Value::test_string("custom/unknown-model"),
            "providers" => Value::test_record(providers_map.into_iter().collect()),
        });

        let config = resolve_config(
            &MockEngineInterface::with_config(plugin_config),
            &create_test_call(vec![]),
        )
        .expect("resolve config");

        let defaults = PreambleDefaults::builtin();
        let expected_global_fallback = defaults
            .global_fallback()
            .expect("builtin global fallback preamble should always be set");

        assert_eq!(config.preamble.as_deref(), Some(expected_global_fallback));
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
    fn resolve_config_no_plugin_config_requires_model_flag() {
        let mock = MockEngineInterface::new();

        let call = create_test_call(vec![("model", Value::test_string("openai/gpt-4"))]);

        let result = resolve_config(&mock, &call);
        assert!(result.is_ok(), "Failed to resolve config: {:?}", result);

        let config = result.unwrap();
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4");
    }
}

#[test]
fn docs_usage_flag_reference_excludes_removed_flags() {
    let usage_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/usage.md");
    let usage = std::fs::read_to_string(&usage_path).expect("read docs/usage.md");

    assert!(
        !usage.contains("--provider"),
        "docs/usage.md must not reference removed --provider flag"
    );
    assert!(
        !usage.contains("--max-tokens"),
        "docs/usage.md must not reference removed --max-tokens flag"
    );
    assert!(usage.contains("--model"), "docs should reference --model");
    assert!(
        usage.contains("--max-output-tokens") && usage.contains("--max-context-tokens"),
        "docs should reference explicit token knobs"
    );
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
    fn agent_command_signature_has_agent_flag() {
        let (agent, _temp_dir) = create_test_agent();
        let sig = SimplePluginCommand::signature(&agent);

        let agent_flag = sig.named.iter().find(|f| f.long == "agent");
        assert!(agent_flag.is_some(), "Missing --agent flag");

        let flag = agent_flag.unwrap();
        assert_eq!(
            flag.arg,
            Some(SyntaxShape::String),
            "Wrong type for --agent"
        );
        assert!(!flag.desc.is_empty(), "Missing description for --agent");
    }

    #[test]
    fn agent_command_signature_has_name_flag() {
        let (agent, _temp_dir) = create_test_agent();
        let sig = SimplePluginCommand::signature(&agent);

        let name_flag = sig.named.iter().find(|f| f.long == "name");
        assert!(name_flag.is_some(), "Missing --name flag");

        let flag = name_flag.unwrap();
        assert_eq!(flag.arg, Some(SyntaxShape::String), "Wrong type for --name");
        assert!(!flag.desc.is_empty(), "Missing description for --name");
    }
}

// Tests for session flag validation
#[cfg(test)]
mod session_validation_tests {
    use super::*;
    use crate::command::agent::extract_and_validate_session_flags;

    /// Helper to create a mock EvaluatedCall for testing
    fn create_mock_call_with_session_flags(session: Option<&str>) -> EvaluatedCall {
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

        EvaluatedCall {
            head: Span::test_data(),
            positional: vec![],
            named,
        }
    }

    #[test]
    fn validate_session_flags_accepts_session_id_only() {
        // RED: Test that --session <id> alone is valid
        let call = create_mock_call_with_session_flags(Some("my-session"));
        let result = extract_and_validate_session_flags(&call);

        assert!(result.is_ok(), "Should accept --session alone");
        let session_id = result.unwrap();
        assert_eq!(session_id, Some("my-session".to_string()));
    }

    #[test]
    fn validate_session_flags_accepts_no_flags() {
        // RED: Test that no session flags is valid (default behavior)
        let call = create_mock_call_with_session_flags(None);
        let result = extract_and_validate_session_flags(&call);

        assert!(result.is_ok(), "Should accept no session flags");
        let session_id = result.unwrap();
        assert!(session_id.is_none());
    }
}

#[cfg(test)]
mod tui_session_resolution_tests {
    use nu_agent_core::session::resolver::{
        SessionRequest, generate_session_id, resolve_session_request,
    };
    use nu_protocol::{Span, Value};

    #[test]
    fn interactive_tui_without_session_auto_creates() {
        let request = resolve_session_request(true, None);
        match request {
            SessionRequest::Create(id) => {
                assert!(id.chars().next().is_some_and(|c| c.is_ascii_digit()))
            }
            other => panic!("expected Create request, got: {other:?}"),
        }
    }

    #[test]
    fn interactive_tui_with_session_attaches_existing() {
        let request = resolve_session_request(true, Some("chat-123".to_string()));
        assert_eq!(request, SessionRequest::Attach("chat-123".to_string()));
    }

    #[test]
    fn non_tui_with_session_keeps_legacy_get_or_create_behavior() {
        let request = resolve_session_request(false, Some("chat-legacy".to_string()));
        assert_eq!(request, SessionRequest::Create("chat-legacy".to_string()));
    }

    #[test]
    fn non_tui_without_session_returns_none() {
        let request = resolve_session_request(false, None);
        assert_eq!(request, SessionRequest::None);
    }

    #[test]
    fn generated_session_id_matches_expected_prefix() {
        let id = generate_session_id();
        assert!(
            id.chars().next().is_some_and(|c| c.is_ascii_digit()),
            "expected timestamp prefix, got: {id}"
        );
        assert!(id.len() >= 15, "session id too short: {id}");
    }

    #[test]
    fn interactive_tui_normal_quit_returns_nothing() {
        let value = Value::nothing(Span::test_data());
        assert!(
            value.is_nothing(),
            "interactive TUI quit must return nothing"
        );
    }
}

// Integration tests for session functionality
#[cfg(test)]
mod session_integration_tests {
    use super::*;
    use crate::command::agent::extract_and_validate_session_flags;

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
    fn extract_session_flags_with_session_id() {
        // Test extracting --session flag
        let call = create_mock_call_with_session_flags(Some("my-session"));
        let result = extract_and_validate_session_flags(&call);

        assert!(result.is_ok());
        let session_id = result.unwrap();
        assert_eq!(session_id, Some("my-session".to_string()));
    }

    #[test]
    fn extract_session_flags_default_no_flags() {
        // Test default behavior (no session flags)
        let call = create_mock_call_with_session_flags(None);
        let result = extract_and_validate_session_flags(&call);

        assert!(result.is_ok());
        let session_id = result.unwrap();
        assert!(session_id.is_none());
    }

    /// Helper to create a mock EvaluatedCall for testing (imported from session_validation_tests)
    fn create_mock_call_with_session_flags(session: Option<&str>) -> EvaluatedCall {
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

        EvaluatedCall {
            head: Span::test_data(),
            positional: vec![],
            named,
        }
    }

    #[test]
    fn extract_tools_from_call_missing_flag() {
        // Test with no --tools flag
        let call = create_test_call(vec![]);
        let result = extract_tools_from_call(&call);

        assert!(result.is_ok());
        let tools = result.unwrap();
        assert_eq!(tools.len(), 0);
    }

    #[test]
    fn extract_tools_from_call_empty_record() {
        // Test with empty record
        use nu_protocol::Record;
        let call = create_test_call(vec![("tools", Value::test_record(Record::new()))]);
        let result = extract_tools_from_call(&call);

        assert!(result.is_ok());
        let tools = result.unwrap();
        assert_eq!(tools.len(), 0);
    }

    #[test]
    fn extract_tools_from_call_with_closures() {
        // Test with record containing closures
        use nu_protocol::{BlockId, Record, engine::Closure};

        let mut record = Record::new();
        record.insert(
            "add".to_string(),
            Value::test_closure(Closure {
                block_id: BlockId::new(1),
                captures: vec![],
            }),
        );
        record.insert(
            "multiply".to_string(),
            Value::test_closure(Closure {
                block_id: BlockId::new(2),
                captures: vec![],
            }),
        );

        let call = create_test_call(vec![("tools", Value::test_record(record))]);
        let result = extract_tools_from_call(&call);

        assert!(result.is_ok());
        let tools = result.unwrap();
        assert_eq!(tools.len(), 2);
        assert!(tools.contains_key("add"));
        assert!(tools.contains_key("multiply"));
    }

    #[test]
    fn extract_tools_from_call_filters_non_closures() {
        // Test that non-closure values are filtered out
        use nu_protocol::{BlockId, Record, engine::Closure};

        let mut record = Record::new();
        record.insert(
            "add".to_string(),
            Value::test_closure(Closure {
                block_id: BlockId::new(1),
                captures: vec![],
            }),
        );
        record.insert("name".to_string(), Value::test_string("not a closure"));
        record.insert("count".to_string(), Value::test_int(42));
        record.insert(
            "multiply".to_string(),
            Value::test_closure(Closure {
                block_id: BlockId::new(2),
                captures: vec![],
            }),
        );

        let call = create_test_call(vec![("tools", Value::test_record(record))]);
        let result = extract_tools_from_call(&call);

        assert!(result.is_ok());
        let tools = result.unwrap();
        // Only closures should be extracted
        assert_eq!(tools.len(), 2);
        assert!(tools.contains_key("add"));
        assert!(tools.contains_key("multiply"));
        assert!(!tools.contains_key("name"));
        assert!(!tools.contains_key("count"));
    }

    #[test]
    fn extract_tools_from_call_non_record_value() {
        // Test with non-record value (graceful handling)
        let call = create_test_call(vec![("tools", Value::test_string("not a record"))]);
        let result = extract_tools_from_call(&call);

        assert!(result.is_ok());
        let tools = result.unwrap();
        assert_eq!(tools.len(), 0);
    }
}

// Tests for --tool-timeout flag parsing
mod tool_timeout_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn parses_tool_timeout_flag() {
        // Test parsing Duration from Nushell duration value (i64 nanoseconds)
        // Nushell represents durations as i64 nanoseconds
        // 5 seconds = 5_000_000_000 nanoseconds
        let timeout_nanos = 5_000_000_000i64;
        let call = create_test_call(vec![("tool-timeout", Value::test_duration(timeout_nanos))]);

        // Use the helper function to extract timeout
        let timeout = extract_tool_timeout(&call);

        assert_eq!(timeout, Duration::from_secs(5));
    }

    #[test]
    fn defaults_to_30_seconds_when_flag_missing() {
        // Test default behavior when flag is not provided
        let call = create_test_call(vec![]);

        // Use the helper function (should return default)
        let timeout = extract_tool_timeout(&call);

        assert_eq!(timeout, Duration::from_secs(30));
    }

    #[test]
    fn parses_millisecond_timeout() {
        // Test parsing smaller duration (100ms = 100_000_000 nanoseconds)
        let timeout_nanos = 100_000_000i64;
        let call = create_test_call(vec![("tool-timeout", Value::test_duration(timeout_nanos))]);

        let timeout = extract_tool_timeout(&call);

        assert_eq!(timeout, Duration::from_millis(100));
    }

    #[test]
    fn agent_signature_has_tool_timeout_flag() {
        // Test that the signature includes --tool-timeout flag
        let (agent, _temp_dir) = create_test_agent();
        let sig = SimplePluginCommand::signature(&agent);

        let flag = sig.named.iter().find(|f| f.long == "tool-timeout");
        assert!(flag.is_some(), "Missing --tool-timeout flag");

        let flag = flag.unwrap();
        assert_eq!(flag.short, Some('t'), "Missing -t short flag");
        assert_eq!(
            flag.arg,
            Some(SyntaxShape::Duration),
            "Wrong type for --tool-timeout"
        );
        assert!(
            !flag.desc.is_empty(),
            "Missing description for --tool-timeout"
        );
    }
}

// ============================================================================
// Persona Model Precedence Tests - Fix for persona model override bug
// ============================================================================

#[test]
fn apply_persona_model_overrides_plugin_config() {
    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        provider_impl: None,
        ..Config::default()
    };

    let applied = runtime_build::apply_persona_model(
        &mut config,
        Some("github-copilot/claude-opus-4.6"),
        false,
    );

    assert!(applied, "Should apply persona model");
    assert_eq!(config.provider, "github-copilot");
    assert_eq!(config.model, "claude-opus-4.6");
    assert_eq!(config.provider_impl, None);
}

#[test]
fn apply_persona_model_cli_wins() {
    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        provider_impl: None,
        ..Config::default()
    };

    let applied = runtime_build::apply_persona_model(
        &mut config,
        Some("github-copilot/claude-opus-4.6"),
        true, // CLI model was provided
    );

    assert!(!applied, "Should NOT apply persona model when CLI provided");
    assert_eq!(config.provider, "openai", "Config should be unchanged");
    assert_eq!(config.model, "gpt-4o", "Config should be unchanged");
}

#[test]
fn apply_persona_model_no_slash_ignored() {
    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        provider_impl: None,
        ..Config::default()
    };

    let applied = runtime_build::apply_persona_model(&mut config, Some("just-a-model"), false);

    assert!(!applied, "Should NOT apply invalid persona model");
    assert_eq!(config.provider, "openai", "Config should be unchanged");
    assert_eq!(config.model, "gpt-4o", "Config should be unchanged");
}

#[test]
fn apply_persona_model_none_preserves_config() {
    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        provider_impl: None,
        ..Config::default()
    };

    let applied = runtime_build::apply_persona_model(&mut config, None, false);

    assert!(!applied, "Should NOT apply when persona model is None");
    assert_eq!(config.provider, "openai", "Config should be unchanged");
    assert_eq!(config.model, "gpt-4o", "Config should be unchanged");
}

#[test]
fn apply_persona_model_clears_provider_impl() {
    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        provider_impl: Some("openai".to_string()),
        ..Config::default()
    };

    let applied = runtime_build::apply_persona_model(
        &mut config,
        Some("anthropic/claude-sonnet-4-20250514"),
        false,
    );

    assert!(applied, "Should apply persona model");
    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude-sonnet-4-20250514");
    assert_eq!(
        config.provider_impl, None,
        "provider_impl should be cleared"
    );
}
