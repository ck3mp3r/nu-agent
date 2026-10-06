use super::test_helpers::{
    create_test_agent, create_test_call, test_model, test_plugin_config, test_provider,
};

use nu_agent_core::config::{ModelConfig, ModelLimits, ProviderConfig};
use nu_plugin::SimplePluginCommand;
use nu_protocol::Value;
use std::collections::HashMap;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ============================================================================
// New Plugin Config Tests - Test provider/model format and --small flag
// ============================================================================

mod new_plugin_config_tests {
    use super::*;
    use crate::command::agent::{picker::format_active_model_identity, runtime_build};

    #[test]
    fn signature_has_model_flag_for_provider_model_format() -> Result<()> {
        let (agent, _temp_dir) = create_test_agent();
        let sig = SimplePluginCommand::signature(&agent);

        let model_flag = sig.named.iter().find(|f| f.long == "model");
        assert!(model_flag.is_some(), "Missing --model flag");

        let flag = model_flag.ok_or("should have --model flag")?;
        assert_eq!(flag.short, Some('m'), "Missing -m short flag");
        assert_eq!(
            flag.arg,
            Some(nu_protocol::SyntaxShape::String),
            "Wrong type for --model"
        );
        // Description should mention provider/model format
        assert!(
            flag.desc.contains("provider/model")
                || flag.desc.contains("provider") && flag.desc.contains("model"),
            "Flag description should mention provider/model format: {}",
            flag.desc
        );
        Ok(())
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
    fn resolve_config_with_new_plugin_config_structure() -> Result<()> {
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

        let config = runtime_build::resolve_with_new_config(plugin_config, &call)
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4");
        assert_eq!(config.api_key, Some("test_key".to_string()));
        assert_eq!(config.temperature, Some(0.7));
        assert_eq!(config.max_context_tokens, Some(128000));
        assert_eq!(config.max_output_tokens, Some(4096));
        Ok(())
    }

    #[test]
    fn resolve_config_accepts_mcp_from_toml_config() -> Result<()> {
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
        let parsed = nu_agent_core::tools::mcp::config::McpConfig::from_toml(&mcp_value)
            .map_err(|e| format!("{e:?}"))?;
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
        Ok(())
    }

    #[test]
    fn resolve_config_with_model_flag_override() -> Result<()> {
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
        let config = result.map_err(|e| format!("{e:?}"))?;
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-3.5-turbo"); // Flag overrides default
        assert_eq!(config.temperature, Some(0.9)); // Model-specific temperature
        Ok(())
    }

    #[test]
    fn resolve_config_with_model_flag_override_for_tui_path_uses_flag_precedence() -> Result<()> {
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
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4o-mini");
        assert_eq!(
            format_active_model_identity(&config.provider, &config.model),
            "openai/gpt-4o-mini"
        );
        Ok(())
    }

    #[test]
    fn resolve_config_uses_models_default() -> Result<()> {
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

        let config = runtime_build::resolve_with_new_config(plugin_config, &call)
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-3.5-turbo"); // Uses models.default
        assert_eq!(config.temperature, Some(1.0)); // Model-specific temperature
        Ok(())
    }

    #[test]
    fn resolve_config_new_flow_resolves_model_preamble_over_provider_preamble() -> Result<()> {
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
                .map_err(|e| format!("{e:?}"))?;

        assert_eq!(config.preamble.as_deref(), Some("model preamble"));
        Ok(())
    }

    #[test]
    fn resolve_config_new_flow_falls_back_to_global_preamble_on_complete_miss() -> Result<()> {
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
                .map_err(|e| format!("{e:?}"))?;

        let defaults = PreambleDefaults::builtin();
        let expected_global_fallback = defaults
            .global_fallback()
            .ok_or("builtin global fallback preamble should be set")?;

        assert_eq!(config.preamble.as_deref(), Some(expected_global_fallback));
        Ok(())
    }

    #[test]
    fn resolve_config_model_flag_overrides_models_default() -> Result<()> {
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

        let config = runtime_build::resolve_with_new_config(plugin_config, &call)
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(config.model, "gpt-4"); // --model wins over models.default
        Ok(())
    }

    #[test]
    fn resolve_config_no_plugin_config_requires_model_flag() -> Result<()> {
        // Literal --model flag with a minimal provider config.
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
        Ok(())
    }
}

#[test]
fn docs_usage_flag_reference_excludes_removed_flags() -> Result<()> {
    let usage_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/usage.md");
    let usage = std::fs::read_to_string(&usage_path).map_err(|e| format!("{e:?}"))?;

    // Extract the agent `## Flag reference` section only — subcommand flags
    // (e.g. `agent models list --provider`) are separate and may legitimately
    // use names the agent command removed.
    let flag_section = usage
        .split("## Flag reference")
        .nth(1)
        .and_then(|s| s.split("## Subcommands").next())
        .unwrap_or("");

    assert!(
        !flag_section.contains("--provider"),
        "agent flag reference must not include removed --provider flag"
    );
    assert!(
        !flag_section.contains("--max-tokens"),
        "agent flag reference must not include removed --max-tokens flag"
    );
    assert!(usage.contains("--model"), "docs should reference --model");
    assert!(
        usage.contains("--max-output-tokens") && usage.contains("--max-context-tokens"),
        "docs should reference explicit token knobs"
    );
    Ok(())
}
