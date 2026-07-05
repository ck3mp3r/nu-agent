use super::picker::*;

#[test]
fn format_active_model_identity_avoids_duplicate_provider_prefix() {
    assert_eq!(
        format_active_model_identity("openai", "openai/gpt-4o-mini"),
        "openai/gpt-4o-mini"
    );
    assert_eq!(
        format_active_model_identity("openai", "gpt-4o-mini"),
        "openai/gpt-4o-mini"
    );
}

#[test]
fn model_picker_catalog_projection_from_plugin_config_is_sorted_and_marks_active() {
    use std::collections::HashMap;

    let mut providers = HashMap::new();

    let mut z_models = HashMap::new();
    z_models.insert(
        "z-model".to_string(),
        nu_agent_core::config::ModelConfig {
            limit: None,
            name: None,
            temperature: None,
            preamble: None,
            tool_call: None,
        },
    );
    providers.insert(
        "z-provider".to_string(),
        nu_agent_core::config::ProviderConfig {
            name: None,
            api_key: None,
            base_url: None,
            provider: None,
            preamble: None,
            models: z_models,
        },
    );

    let mut a_models = HashMap::new();
    a_models.insert(
        "a-model".to_string(),
        nu_agent_core::config::ModelConfig {
            limit: None,
            name: None,
            temperature: None,
            preamble: None,
            tool_call: None,
        },
    );
    providers.insert(
        "a-provider".to_string(),
        nu_agent_core::config::ProviderConfig {
            name: None,
            api_key: None,
            base_url: None,
            provider: None,
            preamble: None,
            models: a_models,
        },
    );

    let plugin_config = nu_agent_core::config::PluginConfig {
        model: "a-provider/a-model".to_string(),
        small_model: None,
        providers,
        compaction: None,
        agents: nu_agent_core::config::AgentsConfig::default(),
        read_timeout_secs: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
    };
    let projected =
        build_model_picker_catalog_from_plugin_config(&plugin_config, "a-provider/a-model");

    assert_eq!(projected.len(), 2);
    assert_eq!(projected[0].identity, "a-provider/a-model");
    assert!(projected[0].active);
    assert_eq!(projected[1].identity, "z-provider/z-model");
    assert!(!projected[1].active);
}

#[test]
fn tui_startup_hydrates_model_picker_catalog_from_cached_plugin_config() {
    use std::collections::HashMap;

    let mut providers = HashMap::new();

    let mut openai_models = HashMap::new();
    openai_models.insert(
        "gpt-4o-mini".to_string(),
        nu_agent_core::config::ModelConfig {
            limit: None,
            name: None,
            temperature: None,
            preamble: None,
            tool_call: None,
        },
    );
    providers.insert(
        "openai".to_string(),
        nu_agent_core::config::ProviderConfig {
            name: None,
            api_key: None,
            base_url: None,
            provider: None,
            preamble: None,
            models: openai_models,
        },
    );

    let plugin_config = nu_agent_core::config::PluginConfig {
        model: "openai/gpt-4o-mini".to_string(),
        small_model: None,
        providers,
        compaction: None,
        agents: nu_agent_core::config::AgentsConfig::default(),
        read_timeout_secs: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
    };

    let catalog = model_picker_catalog_from_cached_startup_plugin_config(
        Some(&plugin_config),
        "openai/gpt-4o-mini",
    );

    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0].identity, "openai/gpt-4o-mini");
    assert!(catalog[0].active);
}
