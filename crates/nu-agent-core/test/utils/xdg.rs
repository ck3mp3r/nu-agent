//! Tests for XDG Base Directory implementation
//!
//! Every test injects an explicit environment map, so no test mutates
//! process-global state and the whole module runs in parallel.

use crate::test_support::env_map;
use crate::utils::env_map::EnvMap;
use crate::utils::xdg::*;
use std::path::PathBuf;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn data_dir_uses_xdg_data_home_when_set() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("XDG_DATA_HOME", "/custom/data")]);

    // -- Exec
    let resolved = data_dir_with(&env);

    // -- Check
    assert_eq!(
        resolved.map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/custom/data")
    );
    Ok(())
}

#[test]
fn data_dir_falls_back_to_home_local_share() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("HOME", "/home/tester")]);

    // -- Exec
    let resolved = data_dir_with(&env);

    // -- Check
    assert_eq!(
        resolved.map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/home/tester").join(".local").join("share")
    );
    Ok(())
}

#[test]
fn data_dir_ignores_empty_xdg_data_home() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("HOME", "/home/tester"), ("XDG_DATA_HOME", "")]);

    // -- Exec
    let resolved = data_dir_with(&env);

    // -- Check
    assert_eq!(
        resolved.map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/home/tester").join(".local").join("share")
    );
    Ok(())
}

#[test]
fn data_dir_fails_when_home_missing() -> Result<()> {
    // -- Setup & Fixtures
    let env = EnvMap::new();

    // -- Exec
    let result = data_dir_with(&env);

    // -- Check
    let err = match result {
        Ok(_) => return Err("data_dir should fail when HOME missing".into()),
        Err(e) => e,
    };
    assert!(matches!(err, XdgError::HomeNotFound));
    Ok(())
}

#[test]
fn cache_dir_uses_xdg_cache_home_when_set() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("XDG_CACHE_HOME", "/custom/cache")]);

    // -- Exec
    let resolved = cache_dir_with(&env);

    // -- Check
    assert_eq!(
        resolved.map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/custom/cache")
    );
    Ok(())
}

#[test]
fn cache_dir_falls_back_to_home_cache() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("HOME", "/home/tester")]);

    // -- Exec
    let resolved = cache_dir_with(&env);

    // -- Check
    assert_eq!(
        resolved.map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/home/tester").join(".cache")
    );
    Ok(())
}

#[test]
fn cache_dir_ignores_empty_xdg_cache_home() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("HOME", "/home/tester"), ("XDG_CACHE_HOME", "")]);

    // -- Exec
    let resolved = cache_dir_with(&env);

    // -- Check
    assert_eq!(
        resolved.map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/home/tester").join(".cache")
    );
    Ok(())
}

#[test]
fn cache_dir_fails_when_home_missing() -> Result<()> {
    // -- Setup & Fixtures
    let env = EnvMap::new();

    // -- Exec
    let result = cache_dir_with(&env);

    // -- Check
    let err = match result {
        Ok(_) => return Err("cache_dir should fail when HOME missing".into()),
        Err(e) => e,
    };
    assert!(matches!(err, XdgError::HomeNotFound));
    Ok(())
}

#[test]
fn config_dir_uses_xdg_config_home_when_set() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("XDG_CONFIG_HOME", "/custom/config")]);

    // -- Exec
    let resolved = config_dir_with(&env);

    // -- Check
    assert_eq!(
        resolved.map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/custom/config")
    );
    Ok(())
}

#[test]
fn config_dir_falls_back_to_home_config() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("HOME", "/home/tester")]);

    // -- Exec
    let resolved = config_dir_with(&env);

    // -- Check
    assert_eq!(
        resolved.map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/home/tester").join(".config")
    );
    Ok(())
}

#[test]
fn config_dir_ignores_empty_xdg_config_home() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("HOME", "/home/tester"), ("XDG_CONFIG_HOME", "")]);

    // -- Exec
    let resolved = config_dir_with(&env);

    // -- Check
    assert_eq!(
        resolved.map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/home/tester").join(".config")
    );
    Ok(())
}

#[test]
fn config_dir_fails_when_home_missing() -> Result<()> {
    // -- Setup & Fixtures
    let env = EnvMap::new();

    // -- Exec
    let result = config_dir_with(&env);

    // -- Check
    let err = match result {
        Ok(_) => return Err("config_dir should fail when HOME missing".into()),
        Err(e) => e,
    };
    assert!(matches!(err, XdgError::HomeNotFound));
    Ok(())
}

#[test]
fn state_dir_uses_xdg_state_home_when_set() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("XDG_STATE_HOME", "/custom/state")]);

    // -- Exec
    let resolved = state_dir_with(&env);

    // -- Check
    assert_eq!(
        resolved.map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/custom/state")
    );
    Ok(())
}

#[test]
fn state_dir_falls_back_to_home_local_state() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("HOME", "/home/tester")]);

    // -- Exec
    let resolved = state_dir_with(&env);

    // -- Check
    assert_eq!(
        resolved.map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/home/tester").join(".local").join("state")
    );
    Ok(())
}

#[test]
fn state_dir_ignores_empty_xdg_state_home() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("HOME", "/home/tester"), ("XDG_STATE_HOME", "")]);

    // -- Exec
    let resolved = state_dir_with(&env);

    // -- Check
    assert_eq!(
        resolved.map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/home/tester").join(".local").join("state")
    );
    Ok(())
}

#[test]
fn state_dir_fails_when_home_missing() -> Result<()> {
    // -- Setup & Fixtures
    let env = EnvMap::new();

    // -- Exec
    let result = state_dir_with(&env);

    // -- Check
    let err = match result {
        Ok(_) => return Err("state_dir should fail when HOME missing".into()),
        Err(e) => e,
    };
    assert!(matches!(err, XdgError::HomeNotFound));
    Ok(())
}

#[test]
fn runtime_dir_uses_xdg_runtime_dir_when_set() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("XDG_RUNTIME_DIR", "/run/user/1000")]);

    // -- Exec
    let resolved = runtime_dir_with(&env);

    // -- Check
    assert_eq!(
        resolved.map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/run/user/1000")
    );
    Ok(())
}

#[test]
fn runtime_dir_fails_when_not_set() -> Result<()> {
    // -- Setup & Fixtures
    let env = EnvMap::new();

    // -- Exec
    let result = runtime_dir_with(&env);

    // -- Check
    let err = match result {
        Ok(_) => return Err("runtime_dir should fail when not set".into()),
        Err(e) => e,
    };
    assert!(matches!(err, XdgError::RuntimeDirNotSet));
    Ok(())
}

#[test]
fn runtime_dir_fails_when_empty() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("XDG_RUNTIME_DIR", "")]);

    // -- Exec
    let result = runtime_dir_with(&env);

    // -- Check
    let err = match result {
        Ok(_) => return Err("runtime_dir should fail when empty".into()),
        Err(e) => e,
    };
    assert!(matches!(err, XdgError::RuntimeDirNotSet));
    Ok(())
}

#[test]
fn xdg_error_display() {
    assert_eq!(
        format!("{}", XdgError::HomeNotFound),
        "HOME environment variable not set"
    );
    assert_eq!(
        format!("{}", XdgError::RuntimeDirNotSet),
        "XDG_RUNTIME_DIR not set and has no fallback"
    );
}

// endregion: --- Tests

// region:    --- Test Support

// endregion: --- Test Support
