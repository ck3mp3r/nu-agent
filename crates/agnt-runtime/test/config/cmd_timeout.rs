//! Tests for the `cmd:` helper timeout precedence.

use std::time::Duration;

use tempfile::TempDir;

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_cmd_timeout_env_overrides_config() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let path = write_config(&dir, "cmd_timeout_secs = 45")?;
    let env = EnvMap::from([("AGENT_CMD_TIMEOUT_SECS".to_string(), "7".to_string())]);

    // -- Exec
    let timeout = resolve_cmd_timeout_with(&path, &env)?;

    // -- Check
    assert_eq!(timeout, Duration::from_secs(7));
    Ok(())
}

#[test]
fn test_cmd_timeout_config_overrides_default() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let path = write_config(&dir, "cmd_timeout_secs = 45")?;

    // -- Exec
    let timeout = resolve_cmd_timeout_with(&path, &EnvMap::new())?;

    // -- Check
    assert_eq!(timeout, Duration::from_secs(45));
    Ok(())
}

#[test]
fn test_cmd_timeout_defaults_when_unset() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let path = write_config(&dir, "")?;

    // -- Exec
    let timeout = resolve_cmd_timeout_with(&path, &EnvMap::new())?;

    // -- Check
    assert_eq!(timeout, DEFAULT_TIMEOUT);
    Ok(())
}

#[test]
fn test_cmd_timeout_defaults_when_file_missing() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let path = dir.path().join("nu-agent").join("config.toml");

    // -- Exec
    let timeout = resolve_cmd_timeout_with(&path, &EnvMap::new())?;

    // -- Check
    assert_eq!(timeout, DEFAULT_TIMEOUT);
    Ok(())
}

// endregion: --- Tests

// region:    --- Test Support

fn write_config(dir: &TempDir, contents: &str) -> Result<std::path::PathBuf> {
    let config_dir = dir.path().join("nu-agent");
    std::fs::create_dir_all(&config_dir)?;
    let path = config_dir.join("config.toml");
    std::fs::write(&path, contents)?;
    Ok(path)
}

// endregion: --- Test Support
