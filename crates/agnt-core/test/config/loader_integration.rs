//! Integration tests for `load_and_resolve_config`.
//!
//! These tests load a real config.toml and probe the OS keychain through
//! `Vault::auto_detect`, so they are gated behind the `integration` feature.

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::load_and_resolve_config;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_loader_load_and_resolve_config_resolves_default_role() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let path = write_config(dir.path(), DEFAULT_CONFIG)?;

    // -- Exec
    let (config, plugin_config) =
        load_and_resolve_config(&path, None).map_err(|e| format!("should resolve config: {e}"))?;

    // -- Check
    assert_eq!(config.provider, "ollama");
    assert_eq!(config.model, "llama3");
    assert_eq!(config.base_url.as_deref(), Some("http://localhost:11434"));
    assert!(
        plugin_config.models.contains_key("default"),
        "the loaded plugin config must carry the default role"
    );
    Ok(())
}

#[test]
fn test_loader_load_and_resolve_config_literal_override_wins_over_default() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let path = write_config(dir.path(), MULTI_ROLE_CONFIG)?;

    // -- Exec
    let (config, _) = load_and_resolve_config(&path, Some("ollama/llama3:70b"))
        .map_err(|e| format!("should resolve config: {e}"))?;

    // -- Check
    assert_eq!(config.provider, "ollama");
    assert_eq!(config.model, "llama3:70b");
    Ok(())
}

#[test]
fn test_loader_load_and_resolve_config_role_label_override() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let path = write_config(dir.path(), MULTI_ROLE_CONFIG)?;

    // -- Exec
    let (config, _) = load_and_resolve_config(&path, Some("heavy"))
        .map_err(|e| format!("should resolve config: {e}"))?;

    // -- Check
    assert_eq!(config.provider, "ollama");
    assert_eq!(config.model, "llama3:70b");
    Ok(())
}

// endregion: --- Tests

// region:    --- Test Support

const DEFAULT_CONFIG: &str = r#"
[models.default]
model = "ollama/llama3"

[providers.ollama]
base_url = "http://localhost:11434"
"#;

const MULTI_ROLE_CONFIG: &str = r#"
[models.default]
model = "ollama/llama3"

[models.heavy]
model = "ollama/llama3:70b"

[providers.ollama]
base_url = "http://localhost:11434"
"#;

fn write_config(dir: &Path, contents: &str) -> Result<PathBuf> {
    let path = dir.join("config.toml");
    std::fs::write(&path, contents)?;
    Ok(path)
}

// endregion: --- Test Support
