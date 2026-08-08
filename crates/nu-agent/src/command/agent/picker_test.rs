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
fn model_picker_catalog_projection_is_sorted_and_marks_active() {
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
        models: [(
            "default".to_string(),
            nu_agent_core::config::ModelRoleConfig {
                model: "a-provider/a-model".to_string(),
                ..nu_agent_core::config::ModelRoleConfig::default()
            },
        )]
        .into(),
        providers,
        compaction: None,
        agents: nu_agent_core::config::AgentsConfig::default(),
        a2a_enabled: None,
        session_store: None,
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    };
    let projected = build_model_picker_catalog(None, &plugin_config, "a-provider/a-model");

    assert_eq!(projected.len(), 2);
    assert_eq!(projected[0].identity, "a-provider/a-model");
    assert!(projected[0].active);
    assert_eq!(projected[1].identity, "z-provider/z-model");
    assert!(!projected[1].active);
}

#[test]
fn tui_startup_hydrates_model_picker_catalog_from_cached_config() {
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
        models: [(
            "default".to_string(),
            nu_agent_core::config::ModelRoleConfig {
                model: "openai/gpt-4o-mini".to_string(),
                ..nu_agent_core::config::ModelRoleConfig::default()
            },
        )]
        .into(),
        providers,
        compaction: None,
        agents: nu_agent_core::config::AgentsConfig::default(),
        a2a_enabled: None,
        session_store: None,
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    };

    let catalog = model_picker_catalog_from_cached_startup_plugin_config(
        Some(&plugin_config),
        "openai/gpt-4o-mini",
    );

    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0].identity, "openai/gpt-4o-mini");
    assert!(catalog[0].active);
}

#[test]
fn build_model_picker_catalog_with_cache_shows_all_models_for_configured_providers() {
    use nu_agent_core::config::models_cache::{ModelLimit, ModelSpec, ModelsCache, ProviderSpec};
    use std::collections::HashMap;

    // Build a ModelsCache with one provider that has 2 models
    let mut cache_providers = HashMap::new();
    let mut cache_models = HashMap::new();
    cache_models.insert(
        "gpt-4o".to_string(),
        ModelSpec {
            id: "gpt-4o".to_string(),
            name: "GPT-4o".to_string(),
            tool_call: true,
            limit: ModelLimit {
                context: 128000,
                output: 16384,
            },
            cost: None,
            modalities: None,
        },
    );
    cache_models.insert(
        "gpt-4o-mini".to_string(),
        ModelSpec {
            id: "gpt-4o-mini".to_string(),
            name: "GPT-4o Mini".to_string(),
            tool_call: true,
            limit: ModelLimit {
                context: 128000,
                output: 16384,
            },
            cost: None,
            modalities: None,
        },
    );
    cache_providers.insert(
        "openai".to_string(),
        ProviderSpec {
            id: "openai".to_string(),
            name: "OpenAI".to_string(),
            env: vec!["OPENAI_API_KEY".to_string()],
            api: Some("https://api.openai.com/v1".to_string()),
            models: cache_models,
        },
    );
    let cache = ModelsCache {
        providers: cache_providers,
    };

    // Build a PluginConfig with only gpt-4o-mini configured
    let mut providers = HashMap::new();
    let mut configured_models = HashMap::new();
    configured_models.insert(
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
            models: configured_models,
        },
    );

    let plugin_config = nu_agent_core::config::PluginConfig {
        models: [(
            "default".to_string(),
            nu_agent_core::config::ModelRoleConfig {
                model: "openai/gpt-4o-mini".to_string(),
                ..nu_agent_core::config::ModelRoleConfig::default()
            },
        )]
        .into(),
        providers,
        compaction: None,
        agents: nu_agent_core::config::AgentsConfig::default(),
        a2a_enabled: None,
        session_store: None,
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    };

    let catalog = build_model_picker_catalog(Some(&cache), &plugin_config, "openai/gpt-4o-mini");

    // Both models from the cache should appear (configured + unconfigured)
    assert_eq!(
        catalog.len(),
        2,
        "should show both models from the configured provider"
    );

    // gpt-4o-mini is configured and active
    let mini = catalog
        .iter()
        .find(|o| o.model == "gpt-4o-mini")
        .expect("gpt-4o-mini should be present");
    assert!(mini.configured, "gpt-4o-mini should be marked configured");
    assert!(mini.active, "gpt-4o-mini should be the active model");
    assert_eq!(
        mini.context_window,
        Some(128000),
        "context_window from cache"
    );
    assert_eq!(mini.max_output, Some(16384), "max_output from cache");
    assert_eq!(
        mini.provider_display_name, "OpenAI",
        "provider_display_name from cache spec"
    );
    assert_eq!(
        mini.display, "OpenAI / GPT-4o Mini",
        "display uses cache names"
    );

    // gpt-4o is NOT configured but still shown (cache enrichment)
    let four = catalog
        .iter()
        .find(|o| o.model == "gpt-4o")
        .expect("gpt-4o should be present from cache");
    assert!(!four.configured, "gpt-4o should NOT be marked configured");
    assert!(!four.active, "gpt-4o should not be active");
    assert_eq!(
        four.context_window,
        Some(128000),
        "context_window from cache"
    );
    assert_eq!(four.max_output, Some(16384), "max_output from cache");
    assert_eq!(
        four.provider_display_name, "OpenAI",
        "provider_display_name from cache spec"
    );
    assert_eq!(four.display, "OpenAI / GPT-4o", "display uses cache names");
}
