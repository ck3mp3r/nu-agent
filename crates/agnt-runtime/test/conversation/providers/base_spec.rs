//! `base_spec`: the active wire's spec, and the cache row's input limit.
//!
//! rig 0.46's `input_limit()` reads `max_input_tokens` before the window
//! (rig-core-0.46.0/src/catalog/spec.rs:422), so the cache branch carries the
//! row's `limit.input` and a built-in catalog row keeps standing as the fact.

use super::*;

use crate::config::models_cache::{ModelLimit, ModelSpec as CacheRow};

/// A cache row for `model`, with the window and output the wire takes from it.
fn cache_row(model: &str, input: Option<u32>) -> CacheRow {
    CacheRow {
        id: model.to_string(),
        name: model.to_string(),
        tool_call: true,
        limit: ModelLimit {
            context: 200_000,
            output: 4_096,
            input,
        },
        cost: None,
        modalities: None,
    }
}

#[test]
fn test_providers_base_spec_carries_the_cache_input_limit() -> Result<()> {
    // -- Setup & Fixtures: a model no built-in row files, whose cache row lists
    // an input limit below its window.
    let id = ProviderId::catalog("openai").ok_or("openai should be a catalog provider id")?;
    let row = cache_row("active-wire-model", Some(120_000));

    // -- Exec
    let spec = base_spec(id, "active-wire-model", Some(&row));

    // -- Check
    assert_eq!(
        spec.max_input_tokens,
        Some(120_000),
        "the cache row's limit.input must reach the wire's spec"
    );
    assert_eq!(
        spec.input_limit(),
        Some(120_000),
        "rig must meter against the input limit, not the wider window"
    );
    assert_eq!(
        spec.context_window,
        Some(200_000),
        "the window is unchanged"
    );
    Ok(())
}

#[test]
fn test_providers_base_spec_without_an_input_row_is_unchanged() -> Result<()> {
    // -- Setup & Fixtures
    let id = ProviderId::catalog("openai").ok_or("openai should be a catalog provider id")?;
    let row = cache_row("active-wire-model", None);

    // -- Exec
    let spec = base_spec(id, "active-wire-model", Some(&row));

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
fn test_providers_base_spec_keeps_the_builtin_catalog_row() -> Result<()> {
    // -- Setup & Fixtures: a model rig's built-in catalog files. Its row is the
    // fact, so the cache row must not be consulted at all — the two windows
    // differ so the check can tell which row the spec came from.
    let id = ProviderId::catalog("anthropic").ok_or("anthropic should be a catalog provider id")?;
    let row = CacheRow {
        id: "claude-haiku-4-5".to_string(),
        name: "a cache row the wire must ignore".to_string(),
        tool_call: true,
        limit: ModelLimit {
            context: 999_000,
            output: 999,
            input: Some(120_000),
        },
        cost: None,
        modalities: None,
    };

    // -- Exec
    let spec = base_spec(id, "claude-haiku-4-5", Some(&row));

    // -- Check
    assert_eq!(
        spec.max_input_tokens, None,
        "a built-in catalog row stands: the cache row is not consulted for it"
    );
    assert_ne!(
        spec.context_window,
        Some(999_000),
        "the wire must keep the catalog's window, not the cache row's"
    );
    Ok(())
}
