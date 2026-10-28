//! Shared fixtures for the runtime_build test modules.
//!
//! Holds the imports, the production re-exports, and the builder helpers every
//! topical runtime_build test file needs. Sub-files pull them in with
//! `use super::*;`.

pub(super) use std::collections::HashMap;

pub(super) use agnt_core::config::{
    Config, ModelConfig, ModelRoleConfig, PluginConfig, ProviderConfig,
};
pub(super) use nu_plugin::EvaluatedCall;
pub(super) use nu_protocol::{Span, Spanned, Value};

pub(super) use crate::command::agent::runtime_build::{
    apply_cli_flags, apply_persona_config, apply_persona_model,
};

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

pub(super) fn call_with_flags(flags: Vec<(&str, Option<Value>)>) -> EvaluatedCall {
    let head = Span::test_data();
    let named = flags
        .into_iter()
        .map(|(name, value)| {
            (
                Spanned {
                    item: name.to_string(),
                    span: head,
                },
                value,
            )
        })
        .collect();

    EvaluatedCall {
        head,
        positional: vec![],
        named,
    }
}

pub(super) fn make_plugin_config(models: HashMap<String, ModelRoleConfig>) -> PluginConfig {
    // Build providers for each unique provider referenced in models
    let mut providers = std::collections::HashMap::new();
    for role in models.values() {
        if let Some((provider_name, _)) = role.model.split_once('/')
            && !providers.contains_key(provider_name)
        {
            providers.insert(
                provider_name.to_string(),
                agnt_core::config::ProviderConfig {
                    name: None,
                    api_key: None,
                    base_url: None,
                    provider: None,
                    preamble: None,
                    catalog: None,
                    models: std::collections::HashMap::new(),
                },
            );
        }
    }
    PluginConfig {
        models,
        providers,
        compaction: None,
        agents: Default::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: None,
        permissions: None,
        mcp: None,
        theme: None,
        hooks: Vec::new(),
        paths: None,
    }
}

pub(super) fn make_provider_config(
    preamble: Option<&str>,
    models: HashMap<String, ModelConfig>,
) -> ProviderConfig {
    ProviderConfig {
        name: None,
        api_key: None,
        base_url: None,
        provider: None,
        preamble: preamble.map(|s| s.to_string()),
        catalog: None,
        models,
    }
}

pub(super) fn make_model_config(preamble: Option<&str>) -> ModelConfig {
    ModelConfig {
        limit: None,
        name: None,
        temperature: None,
        preamble: preamble.map(|s| s.to_string()),
        tool_call: None,
        pricing: None,
    }
}

pub(super) fn make_plugin_config_with_providers(
    models: HashMap<String, ModelRoleConfig>,
    providers: HashMap<String, ProviderConfig>,
) -> PluginConfig {
    PluginConfig {
        models,
        providers,
        compaction: None,
        agents: Default::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: None,
        permissions: None,
        mcp: None,
        theme: None,
        hooks: Vec::new(),
        paths: None,
    }
}

// endregion: --- Test Support
