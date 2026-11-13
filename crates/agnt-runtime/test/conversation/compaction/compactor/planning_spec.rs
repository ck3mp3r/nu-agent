//! `resolve_model_spec_for`: the planning spec's input limit.
//!
//! rig 0.46 meters against `ModelSpec::input_limit()`, which reads
//! `max_input_tokens` before the window (rig-core-0.46.0/src/catalog/spec.rs:422).
//! The resolver therefore carries the models-cache row's `limit.input` into the
//! spec, keeps a row that lists none exactly as it was, and lets rig's built-in
//! catalog stand for a model it files.

use super::*;

use crate::config::PluginConfig;
use crate::config::models_cache::{
    ModelLimit as CacheLimit, ModelSpec as CacheModel, ModelsCache, ProviderSpec,
};
use crate::conversation::compaction::compactor::resolve_model_spec_for;

/// A config naming `model`, with the window and output a cache row would fill.
fn config_for(provider: &str, model: &str) -> crate::config::Config {
    crate::config::Config {
        provider: provider.to_string(),
        model: model.to_string(),
        max_context_tokens: Some(200_000),
        max_output_tokens: Some(4_096),
        ..crate::config::Config::default()
    }
}

/// A plugin config whose models cache holds one row for `provider`/`model`.
fn plugin_config_with(provider: &str, model: &str, input: Option<u32>) -> PluginConfig {
    let row = CacheModel {
        id: model.to_string(),
        name: model.to_string(),
        tool_call: true,
        limit: CacheLimit {
            context: 200_000,
            output: 4_096,
            input,
        },
        cost: None,
        modalities: None,
    };
    let section = ProviderSpec {
        id: provider.to_string(),
        name: provider.to_string(),
        env: Vec::new(),
        api: None,
        models: std::collections::HashMap::from([(model.to_string(), row)]),
    };
    PluginConfig {
        models_cache: Some(ModelsCache {
            providers: std::collections::HashMap::from([(provider.to_string(), section)]),
        }),
        ..PluginConfig::default()
    }
}

#[test]
fn test_conversation_compaction_planning_spec_carries_the_cache_input_limit() -> Result<()> {
    // -- Setup & Fixtures: a model the built-in catalog does not file, whose
    // cache row lists an input limit below its window.
    let config = config_for("openai", "planning-spec-model");
    let plugin_config = plugin_config_with("openai", "planning-spec-model", Some(120_000));

    // -- Exec
    let spec = resolve_model_spec_for(&config, Some(&plugin_config))
        .ok_or("openai should resolve to a registered provider id")?;

    // -- Check
    assert_eq!(
        spec.max_input_tokens,
        Some(120_000),
        "the cache row's limit.input must reach the planning spec"
    );
    assert_eq!(
        spec.input_limit(),
        Some(120_000),
        "rig must meter against the input limit, not the wider window"
    );
    assert_eq!(
        spec.context_window,
        Some(200_000),
        "the window keeps its own value"
    );
    Ok(())
}

#[test]
fn test_conversation_compaction_planning_spec_without_an_input_row_is_unchanged() -> Result<()> {
    // -- Setup & Fixtures
    let config = config_for("openai", "planning-spec-model");
    let plugin_config = plugin_config_with("openai", "planning-spec-model", None);

    // -- Exec
    let spec = resolve_model_spec_for(&config, Some(&plugin_config))
        .ok_or("openai should resolve to a registered provider id")?;

    // -- Check
    assert_eq!(
        spec.max_input_tokens, None,
        "a row without limit.input must leave the spec's input limit unset"
    );
    assert_eq!(
        spec.input_limit(),
        Some(200_000),
        "without an input limit rig degrades to the window, as before"
    );
    Ok(())
}

#[test]
fn test_conversation_compaction_planning_spec_keeps_the_builtin_catalog_row() -> Result<()> {
    // -- Setup & Fixtures: a model rig's built-in catalog files. Its row is the
    // fact, so the cache row's input limit must not be consulted.
    let config = config_for("anthropic", "claude-haiku-4-5");
    let plugin_config = plugin_config_with("anthropic", "claude-haiku-4-5", Some(120_000));

    // -- Exec
    let spec = resolve_model_spec_for(&config, Some(&plugin_config))
        .ok_or("anthropic should resolve to a registered provider id")?;

    // -- Check
    assert_eq!(
        spec.max_input_tokens, None,
        "a built-in catalog row stands: the cache row is not consulted for it"
    );
    Ok(())
}
