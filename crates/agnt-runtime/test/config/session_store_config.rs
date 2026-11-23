//! The `[session_store]` table: backend selection and refusal of dead keys.

use tempfile::TempDir;

use crate::config::toml_config::load_from;
use crate::config::{ModelRoleConfig, StoreType, TomlConfigError};
use crate::utils::env_map::EnvMap;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// WHEN `[session_store]` sets `store_type = "memory"`, THE load SHALL carry it
/// and THE resolve SHALL forward it into the runtime config.
#[test]
fn session_store_config_forwards_store_type() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, path) = write_config("[session_store]\nstore_type = \"memory\"\n")?;

    // -- Exec
    let plugin_config = load_from(&path).map_err(|e| format!("load should succeed: {e}"))?;
    let resolved = plugin_config
        .resolve_model_with(&role("openai/gpt-4"), &EnvMap::new())
        .map_err(|e| format!("resolve should succeed: {e}"))?;

    // -- Check
    assert_eq!(
        plugin_config
            .session_store
            .as_ref()
            .map(|store| store.store_type),
        Some(StoreType::Memory),
        "the parsed table carries the memory backend"
    );
    assert_eq!(
        resolved.session_store_type,
        Some(StoreType::Memory),
        "the resolver forwards store_type into the runtime config"
    );
    Ok(())
}

/// WHEN `[session_store]` sets the removed `path` key, THEN THE load SHALL refuse
/// it and name `path`: the field was never read, so a stale key must not pass
/// silently.
#[test]
fn session_store_config_refuses_the_removed_path_key() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, path) = write_config("[session_store]\npath = \"/x\"\n")?;

    // -- Exec
    let error = match load_from(&path) {
        Ok(_) => return Err("load must refuse the removed `path` key".into()),
        Err(e) => e,
    };

    // -- Check
    assert!(
        matches!(error, TomlConfigError::Parse(_)),
        "a dead key is refused as a parse error, got: {error}"
    );
    assert!(
        error.to_string().contains("path"),
        "the refusal names the removed key, got: {error}"
    );
    Ok(())
}

// region:    --- Test Support

/// A role config pointing at `spec`.
fn role(spec: &str) -> ModelRoleConfig {
    ModelRoleConfig {
        model: spec.to_string(),
        ..ModelRoleConfig::default()
    }
}

/// Write `contents` to a temp `config.toml` and return the dir that owns it.
fn write_config(contents: &str) -> Result<(TempDir, std::path::PathBuf)> {
    let dir = TempDir::new().map_err(|e| format!("failed to create temp dir: {e}"))?;
    let path = dir.path().join("config.toml");
    std::fs::write(&path, contents).map_err(|e| format!("failed to write config: {e}"))?;
    Ok((dir, path))
}

// endregion: --- Test Support
