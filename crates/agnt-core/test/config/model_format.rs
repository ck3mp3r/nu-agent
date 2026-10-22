//! Multi-part model-format tests (`provider/backend/model`).

use std::collections::HashMap;

use crate::config::{AgentsConfig, ModelRoleConfig, PluginConfig, ProviderConfig};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

struct Case {
    name: &'static str,
    plugin_config: PluginConfig,
    model: &'static str,
    provider: &'static str,
    expected_model: &'static str,
    api_key: Option<&'static str>,
    base_url: Option<&'static str>,
}

#[test]
fn resolve_model_handles_multi_part_format() -> Result<()> {
    // -- Setup & Fixtures
    let cases = vec![
        Case {
            name: "two-part format",
            plugin_config: plugin_config_with_provider("openai", Some("sk-test123"), None),
            model: "openai/gpt-4",
            provider: "openai",
            expected_model: "gpt-4",
            api_key: Some("sk-test123"),
            base_url: None,
        },
        Case {
            name: "three-part format splits on the first slash",
            plugin_config: plugin_config_with_provider(
                "github-copilot",
                Some("test-key"),
                Some("https://api.githubcopilot.com"),
            ),
            model: "github-copilot/anthropic/claude-sonnet-4-20250514",
            provider: "github-copilot",
            expected_model: "anthropic/claude-sonnet-4-20250514",
            api_key: Some("test-key"),
            base_url: Some("https://api.githubcopilot.com"),
        },
        Case {
            name: "three-part format with an openai backend",
            plugin_config: plugin_config_with_provider(
                "github-copilot",
                Some("test-key"),
                Some("https://api.githubcopilot.com"),
            ),
            model: "github-copilot/openai/gpt-4o-mini",
            provider: "github-copilot",
            expected_model: "openai/gpt-4o-mini",
            api_key: Some("test-key"),
            base_url: Some("https://api.githubcopilot.com"),
        },
        Case {
            name: "simple two-part format",
            plugin_config: plugin_config_with_provider(
                "openai",
                Some("test-key"),
                Some("https://api.githubcopilot.com"),
            ),
            model: "openai/gpt-4",
            provider: "openai",
            expected_model: "gpt-4",
            api_key: Some("test-key"),
            base_url: Some("https://api.githubcopilot.com"),
        },
    ];

    // -- Exec & Check
    for case in cases {
        let role_config = ModelRoleConfig {
            model: case.model.to_string(),
            ..ModelRoleConfig::default()
        };
        let config = case
            .plugin_config
            .resolve_model(&role_config)
            .map_err(|e| format!("{}: {e:?}", case.name))?;
        assert_eq!(config.provider, case.provider, "{}", case.name);
        assert_eq!(config.model, case.expected_model, "{}", case.name);
        assert_eq!(config.api_key.as_deref(), case.api_key, "{}", case.name);
        assert_eq!(config.base_url.as_deref(), case.base_url, "{}", case.name);
    }
    Ok(())
}

#[test]
fn resolve_model_rejects_empty_parts() -> Result<()> {
    // -- Setup & Fixtures
    let plugin_config = plugin_config_with_provider("openai", None, None);
    let cases = ["/model", "provider/", "/"];

    // -- Exec & Check
    for model in cases {
        let role_config = ModelRoleConfig {
            model: model.to_string(),
            ..ModelRoleConfig::default()
        };
        let err = match plugin_config.resolve_model(&role_config) {
            Ok(_) => return Err(format!("{model} should fail resolution").into()),
            Err(e) => e,
        };
        assert!(err.contains("cannot be empty"), "{model}: got {err}");
    }
    Ok(())
}

// region:    --- Test Support

fn plugin_config_with_provider(
    provider: &str,
    api_key: Option<&str>,
    base_url: Option<&str>,
) -> PluginConfig {
    let mut providers = HashMap::new();
    providers.insert(
        provider.to_string(),
        ProviderConfig {
            name: None,
            api_key: api_key.map(str::to_string),
            base_url: base_url.map(str::to_string),
            provider: None,
            preamble: None,
            catalog: None,
            models: HashMap::new(),
        },
    );
    PluginConfig {
        models: HashMap::new(),
        providers,
        compaction: None,
        agents: AgentsConfig::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: None,
        permissions: None,
        mcp: None,
        theme: None,
    }
}

// endregion: --- Test Support
