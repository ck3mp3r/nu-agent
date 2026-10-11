//! Shared fixtures for the config test modules.

use std::collections::HashMap;

use crate::config::{AgentsConfig, ModelRoleConfig, ModelsCache, PluginConfig, ProviderConfig};

// -- Test Support: build a PluginConfig with a models cache entry for openai/gpt-4
// (cache limit.output = 4096, matching models_cache_test::make_test_cache).

pub(super) fn plugin_config_with_cache() -> PluginConfig {
    let mut providers = HashMap::new();
    providers.insert(
        "openai".to_string(),
        ProviderConfig {
            name: None,
            api_key: None,
            base_url: None,
            provider: None,
            preamble: None,
            models: HashMap::new(),
        },
    );
    let mut models = HashMap::new();
    models.insert(
        "default".to_string(),
        ModelRoleConfig {
            model: "openai/gpt-4".to_string(),
            ..ModelRoleConfig::default()
        },
    );
    let mut cache_providers = HashMap::new();
    cache_providers.insert(
        "openai".to_string(),
        crate::config::models_cache::ProviderSpec {
            id: "openai".to_string(),
            name: "OpenAI".to_string(),
            env: vec![],
            api: None,
            models: HashMap::from([(
                "gpt-4".to_string(),
                crate::config::models_cache::ModelSpec {
                    id: "gpt-4".to_string(),
                    name: "GPT-4".to_string(),
                    tool_call: true,
                    limit: crate::config::models_cache::ModelLimit {
                        context: 128000,
                        output: 4096,
                    },
                    cost: None,
                    modalities: None,
                },
            )]),
        },
    );
    PluginConfig {
        models,
        providers,
        compaction: None,
        agents: AgentsConfig::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: Some(ModelsCache {
            providers: cache_providers,
        }),
        permissions: None,
        mcp: None,
        theme: None,
    }
}
