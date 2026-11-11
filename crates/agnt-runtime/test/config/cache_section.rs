//! `PluginConfig::cache_section`: the one place a block's models-cache section
//! is resolved.

use super::*;

use std::collections::HashMap;

use crate::config::ProviderConfig;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

fn plugin_config(providers: &[(&str, Option<&str>)]) -> PluginConfig {
    let mut map = HashMap::new();
    for (name, catalog) in providers {
        map.insert(
            (*name).to_string(),
            ProviderConfig {
                catalog: catalog.map(str::to_string),
                ..ProviderConfig::default()
            },
        );
    }
    PluginConfig {
        providers: map,
        ..PluginConfig::default()
    }
}

// endregion: --- Test Support

// region:    --- cache_section

/// A declared catalog wins over the block name.
#[test]
fn test_cache_section_declared_catalog_wins() -> Result<()> {
    // -- Setup & Fixtures
    let config = plugin_config(&[("ollama", Some("ollama-cloud"))]);

    // -- Exec & Check
    assert_eq!(
        config.cache_section("ollama"),
        "ollama-cloud",
        "the declared catalog must be the section key"
    );
    Ok(())
}

/// A block with no declared catalog is its own section key.
#[test]
fn test_cache_section_falls_back_to_the_block_name() -> Result<()> {
    // -- Setup & Fixtures
    let config = plugin_config(&[("openai", None)]);

    // -- Exec & Check
    assert_eq!(
        config.cache_section("openai"),
        "openai",
        "an undeclared block name must be its own section key"
    );
    Ok(())
}

/// A provider with no block at all is its own section key.
#[test]
fn test_cache_section_absent_block_is_its_own_key() -> Result<()> {
    // -- Setup & Fixtures: the config declares no block for `anthropic`.
    let config = plugin_config(&[("openai", None)]);

    // -- Exec & Check
    assert_eq!(
        config.cache_section("anthropic"),
        "anthropic",
        "a provider with no block must be its own section key"
    );
    Ok(())
}

/// No name mapping exists: a rig vendor name reaches no other section.
#[test]
fn test_cache_section_maps_no_names() -> Result<()> {
    // -- Setup & Fixtures: the block declares nothing.
    let config = plugin_config(&[("copilot", None), ("ollama", None)]);

    // -- Exec & Check
    assert_eq!(config.cache_section("copilot"), "copilot");
    assert_eq!(config.cache_section("ollama"), "ollama");
    Ok(())
}

// endregion: --- cache_section
