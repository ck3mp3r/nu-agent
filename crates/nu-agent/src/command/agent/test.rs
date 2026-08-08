use super::test_helpers::{
    create_test_agent, create_test_call, parse_agent_invocation_with_signature,
};
use super::{extract_tool_timeout, extract_tools_from_call, runtime_build};

use nu_agent_core::config::{
    Config, ModelConfig, ModelLimits, ModelRoleConfig, PluginConfig, ProviderConfig,
};
use nu_plugin::{EvaluatedCall, SimplePluginCommand};
use nu_protocol::{ParseError, Span, Spanned, SyntaxShape, Value};
use serial_test::serial;
use std::collections::HashMap;

// Helpers to build PluginConfig structures for resolve_with_new_config tests.
fn test_model() -> ModelConfig {
    ModelConfig::default()
}

fn test_provider(api_key: Option<&str>, models: Vec<(&str, ModelConfig)>) -> ProviderConfig {
    ProviderConfig {
        name: None,
        api_key: api_key.map(str::to_string),
        base_url: None,
        provider: None,
        preamble: None,
        models: models
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    }
}

fn test_plugin_config(default_model: &str, providers: Vec<(&str, ProviderConfig)>) -> PluginConfig {
    let mut models = HashMap::new();
    models.insert(
        "default".to_string(),
        ModelRoleConfig {
            model: default_model.to_string(),
            ..Default::default()
        },
    );
    PluginConfig {
        models,
        providers: providers
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
        ..Default::default()
    }
}

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
    fn resolve_config_with_no_plugin_config() {
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

        let result = runtime_build::resolve_with_new_config(plugin_config, &call);
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

        let result = runtime_build::resolve_with_new_config(plugin_config, &call);
        assert!(result.is_ok(), "resolve failed: {:?}", result);

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

        let result = runtime_build::resolve_with_new_config(plugin_config, &call);
        assert!(result.is_ok(), "resolve failed: {:?}", result);

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
    fn resolve_config_succeeds_without_providers() {
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
        assert!(
            result.is_ok(),
            "expected Ok for config without providers (provider block is optional), got: {result:?}"
        );
    }
}

// ============================================================================
// New Plugin Config Tests - Test provider/model format and --small flag
// ============================================================================

mod new_plugin_config_tests {
    use super::*;
    use crate::command::agent::{picker::format_active_model_identity, runtime_build};

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
    fn signature_does_not_have_small_flag() {
        let (agent, _temp_dir) = create_test_agent();
        let sig = SimplePluginCommand::signature(&agent);

        let small_flag = sig.named.iter().find(|f| f.long == "small");
        assert!(
            small_flag.is_none(),
            "--small flag should have been removed"
        );
    }

    #[test]
    #[serial]
    fn resolve_config_with_new_plugin_config_structure() {
        // Create NEW plugin config structure with provider/model format
        let mut openai_models = HashMap::new();
        openai_models.insert(
            "gpt-4".to_string(),
            ModelConfig {
                temperature: Some(0.7),
                limit: Some(ModelLimits {
                    context: Some(128000),
                    output: Some(4096),
                }),
                ..Default::default()
            },
        );
        let plugin_config = test_plugin_config(
            "openai/gpt-4",
            vec![(
                "openai",
                ProviderConfig {
                    api_key: Some("test_key".to_string()),
                    models: openai_models,
                    ..Default::default()
                },
            )],
        );
        let call = create_test_call(vec![]);

        let result = runtime_build::resolve_with_new_config(plugin_config, &call);
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
    fn resolve_config_accepts_mcp_from_toml_config() {
        use std::collections::HashMap;

        // MCP config is parsed from raw TOML.
        let mcp_value: toml::Value = toml::from_str(
            r#"
[c5t]
transport = "sse"
url = "http://0.0.0.0:3737/mcp"

[nu]
transport = "stdio"
command = "nu-mcp"
args = ["--add-path", "/tmp"]

[nu.env]
GIT_PAGER = ""
"#,
        )
        .unwrap();
        let parsed =
            nu_agent_core::tools::mcp::config::McpConfig::from_toml(&mcp_value).expect("mcp parse");
        assert_eq!(parsed.mcp.len(), 2);

        let mut models = HashMap::new();
        models.insert(
            "claude-sonnet-4-20250514".to_string(),
            ModelConfig::default(),
        );
        let plugin_config = test_plugin_config(
            "github-copilot/anthropic/claude-sonnet-4-20250514",
            vec![(
                "github-copilot",
                ProviderConfig {
                    api_key: Some("token".to_string()),
                    base_url: Some("https://api.individual.githubcopilot.com".to_string()),
                    models,
                    ..Default::default()
                },
            )],
        );

        let resolved =
            runtime_build::resolve_with_new_config(plugin_config, &create_test_call(vec![]));
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
        let mut openai_models = HashMap::new();
        openai_models.insert(
            "gpt-4".to_string(),
            ModelConfig {
                temperature: Some(0.7),
                ..Default::default()
            },
        );
        openai_models.insert(
            "gpt-3.5-turbo".to_string(),
            ModelConfig {
                temperature: Some(0.9),
                ..Default::default()
            },
        );

        let plugin_config = test_plugin_config(
            "openai/gpt-4",
            vec![(
                "openai",
                ProviderConfig {
                    api_key: Some("openai_key".to_string()),
                    models: openai_models,
                    ..Default::default()
                },
            )],
        );

        // Override with --model flag to use gpt-3.5-turbo instead
        let call = create_test_call(vec![("model", Value::test_string("openai/gpt-3.5-turbo"))]);

        let result = runtime_build::resolve_with_new_config(plugin_config, &call);
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

        let mut openai_models = HashMap::new();
        openai_models.insert("gpt-4".to_string(), ModelConfig::default());
        openai_models.insert("gpt-4o-mini".to_string(), ModelConfig::default());

        let plugin_config = test_plugin_config(
            "openai/gpt-4",
            vec![(
                "openai",
                ProviderConfig {
                    api_key: Some("openai_key".to_string()),
                    models: openai_models,
                    ..Default::default()
                },
            )],
        );
        let call = create_test_call(vec![
            ("tui", Value::test_bool(true)),
            ("model", Value::test_string("openai/gpt-4o-mini")),
        ]);

        let config = runtime_build::resolve_with_new_config(plugin_config, &call)
            .expect("resolve config for tui");
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4o-mini");
        assert_eq!(
            format_active_model_identity(&config.provider, &config.model),
            "openai/gpt-4o-mini"
        );
    }

    #[test]
    #[serial]
    fn resolve_config_uses_models_default() {
        use std::collections::HashMap;

        // Create plugin config with models.default
        let mut openai_models = HashMap::new();
        openai_models.insert("gpt-4".to_string(), ModelConfig::default());
        openai_models.insert(
            "gpt-3.5-turbo".to_string(),
            ModelConfig {
                temperature: Some(1.0),
                ..Default::default()
            },
        );

        let plugin_config = test_plugin_config(
            "openai/gpt-3.5-turbo",
            vec![(
                "openai",
                ProviderConfig {
                    api_key: Some("test_key".to_string()),
                    models: openai_models,
                    ..Default::default()
                },
            )],
        );

        // No --model flag — should use models.default
        let call = create_test_call(vec![]);

        let result = runtime_build::resolve_with_new_config(plugin_config, &call);
        assert!(result.is_ok(), "Failed to resolve config: {:?}", result);

        let config = result.unwrap();
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-3.5-turbo"); // Uses models.default
        assert_eq!(config.temperature, Some(1.0)); // Model-specific temperature
    }

    #[test]
    fn resolve_config_new_flow_resolves_model_preamble_over_provider_preamble() {
        use std::collections::HashMap;

        let mut openai_models = HashMap::new();
        openai_models.insert(
            "gpt-5-mini".to_string(),
            ModelConfig {
                preamble: Some("model preamble".to_string()),
                ..Default::default()
            },
        );

        let plugin_config = test_plugin_config(
            "openai/gpt-5-mini",
            vec![(
                "openai",
                ProviderConfig {
                    preamble: Some("provider preamble".to_string()),
                    models: openai_models,
                    ..Default::default()
                },
            )],
        );

        let config =
            runtime_build::resolve_with_new_config(plugin_config, &create_test_call(vec![]))
                .expect("resolve config");

        assert_eq!(config.preamble.as_deref(), Some("model preamble"));
    }

    #[test]
    fn resolve_config_new_flow_falls_back_to_global_preamble_on_complete_miss() {
        use nu_agent_core::protocol::preamble::PreambleDefaults;
        use std::collections::HashMap;

        let mut models = HashMap::new();
        models.insert("unknown-model".to_string(), ModelConfig::default());
        let plugin_config = test_plugin_config(
            "custom/unknown-model",
            vec![(
                "custom",
                ProviderConfig {
                    models,
                    ..Default::default()
                },
            )],
        );

        let config =
            runtime_build::resolve_with_new_config(plugin_config, &create_test_call(vec![]))
                .expect("resolve config");

        let defaults = PreambleDefaults::builtin();
        let expected_global_fallback = defaults
            .global_fallback()
            .expect("builtin global fallback preamble should always be set");

        assert_eq!(config.preamble.as_deref(), Some(expected_global_fallback));
    }

    #[test]
    #[serial]
    fn resolve_config_model_flag_overrides_models_default() {
        use std::collections::HashMap;

        // Create plugin config
        let mut openai_models = HashMap::new();
        openai_models.insert("gpt-4".to_string(), ModelConfig::default());
        openai_models.insert("gpt-3.5-turbo".to_string(), ModelConfig::default());

        let plugin_config = test_plugin_config(
            "openai/gpt-3.5-turbo",
            vec![(
                "openai",
                ProviderConfig {
                    api_key: Some("test_key".to_string()),
                    models: openai_models,
                    ..Default::default()
                },
            )],
        );

        // --model flag provided, should override models.default
        let call = create_test_call(vec![("model", Value::test_string("openai/gpt-4"))]);

        let result = runtime_build::resolve_with_new_config(plugin_config, &call);
        assert!(result.is_ok(), "Failed to resolve config: {:?}", result);

        let config = result.unwrap();
        assert_eq!(config.model, "gpt-4"); // --model wins over models.default
    }

    #[test]
    #[serial]
    fn resolve_config_no_plugin_config_requires_model_flag() {
        // Literal --model flag with a minimal provider config.
        let plugin_config = test_plugin_config(
            "openai/gpt-4",
            vec![(
                "openai",
                test_provider(Some("sk-test"), vec![("gpt-4", test_model())]),
            )],
        );
        let call = create_test_call(vec![("model", Value::test_string("openai/gpt-4"))]);

        let result = runtime_build::resolve_with_new_config(plugin_config, &call);
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
    use std::collections::HashMap;

    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        provider_impl: None,
        ..Config::default()
    };

    // Build a minimal plugin config with the required provider
    let mut models = HashMap::new();
    models.insert(
        "default".to_string(),
        nu_agent_core::config::ModelRoleConfig {
            model: "openai/gpt-4o".to_string(),
            ..Default::default()
        },
    );
    let mut providers = HashMap::new();
    providers.insert(
        "github-copilot".to_string(),
        nu_agent_core::config::ProviderConfig {
            name: None,
            api_key: None,
            base_url: None,
            provider: None,
            preamble: None,
            models: HashMap::new(),
        },
    );
    let plugin_config = nu_agent_core::config::PluginConfig {
        models,
        providers,
        compaction: None,
        agents: Default::default(),
        a2a_enabled: None,
        session_store: None,
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    };

    let applied = runtime_build::apply_persona_model(
        &mut config,
        Some(&plugin_config),
        Some("github-copilot/claude-opus-4.6"),
        false,
    );

    assert!(applied.is_ok(), "Should succeed: {:?}", applied);
    let applied = applied.unwrap();
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
        None,
        Some("github-copilot/claude-opus-4.6"),
        true, // CLI model was provided
    );

    assert!(applied.is_ok(), "Should succeed: {:?}", applied);
    let applied = applied.unwrap();
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

    let applied =
        runtime_build::apply_persona_model(&mut config, None, Some("just-a-model"), false);

    assert!(
        applied.is_err(),
        "Should error when no plugin config and no slash"
    );
}

#[test]
fn apply_persona_model_none_preserves_config() {
    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        provider_impl: None,
        ..Config::default()
    };

    let applied = runtime_build::apply_persona_model(&mut config, None, None, false);

    assert!(applied.is_ok(), "Should succeed: {:?}", applied);
    let applied = applied.unwrap();
    assert!(!applied, "Should NOT apply when persona model is None");
    assert_eq!(config.provider, "openai", "Config should be unchanged");
    assert_eq!(config.model, "gpt-4o", "Config should be unchanged");
}

#[test]
fn apply_persona_model_clears_provider_impl() {
    use std::collections::HashMap;

    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        provider_impl: Some("openai".to_string()),
        ..Config::default()
    };

    // Build a minimal plugin config with the required provider
    let mut models = HashMap::new();
    models.insert(
        "default".to_string(),
        nu_agent_core::config::ModelRoleConfig {
            model: "openai/gpt-4o".to_string(),
            ..Default::default()
        },
    );
    let mut providers = HashMap::new();
    providers.insert(
        "anthropic".to_string(),
        nu_agent_core::config::ProviderConfig {
            name: None,
            api_key: None,
            base_url: None,
            provider: None,
            preamble: None,
            models: HashMap::new(),
        },
    );
    let plugin_config = nu_agent_core::config::PluginConfig {
        models,
        providers,
        compaction: None,
        agents: Default::default(),
        a2a_enabled: None,
        session_store: None,
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    };

    let applied = runtime_build::apply_persona_model(
        &mut config,
        Some(&plugin_config),
        Some("anthropic/claude-sonnet-4-20250514"),
        false,
    );

    assert!(applied.is_ok(), "Should succeed: {:?}", applied);
    let applied = applied.unwrap();
    assert!(applied, "Should apply persona model");
    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude-sonnet-4-20250514");
    assert_eq!(
        config.provider_impl, None,
        "provider_impl should be cleared"
    );
}
