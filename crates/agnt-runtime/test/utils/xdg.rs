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
fn data_dir_resolution() -> Result<()> {
    // -- Case 1: XDG_DATA_HOME wins when set
    let env = env_map(&[("XDG_DATA_HOME", "/custom/data")]);
    assert_eq!(
        data_dir_with(&env).map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/custom/data")
    );

    // -- Case 2: falls back to $HOME/.local/share
    let env = env_map(&[("HOME", "/home/tester")]);
    assert_eq!(
        data_dir_with(&env).map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/home/tester").join(".local").join("share")
    );

    // -- Case 3: an empty XDG_DATA_HOME is ignored
    let env = env_map(&[("HOME", "/home/tester"), ("XDG_DATA_HOME", "")]);
    assert_eq!(
        data_dir_with(&env).map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/home/tester").join(".local").join("share")
    );

    // -- Case 4: a missing HOME fails
    let err = match data_dir_with(&EnvMap::new()) {
        Ok(_) => return Err("data_dir should fail when HOME missing".into()),
        Err(e) => e,
    };
    assert!(matches!(err, XdgError::HomeNotFound));
    Ok(())
}

#[test]
fn cache_dir_resolution() -> Result<()> {
    // -- Case 1: XDG_CACHE_HOME wins when set
    let env = env_map(&[("XDG_CACHE_HOME", "/custom/cache")]);
    assert_eq!(
        cache_dir_with(&env).map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/custom/cache")
    );

    // -- Case 2: falls back to $HOME/.cache
    let env = env_map(&[("HOME", "/home/tester")]);
    assert_eq!(
        cache_dir_with(&env).map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/home/tester").join(".cache")
    );

    // -- Case 3: an empty XDG_CACHE_HOME is ignored
    let env = env_map(&[("HOME", "/home/tester"), ("XDG_CACHE_HOME", "")]);
    assert_eq!(
        cache_dir_with(&env).map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/home/tester").join(".cache")
    );

    // -- Case 4: a missing HOME fails
    let err = match cache_dir_with(&EnvMap::new()) {
        Ok(_) => return Err("cache_dir should fail when HOME missing".into()),
        Err(e) => e,
    };
    assert!(matches!(err, XdgError::HomeNotFound));
    Ok(())
}

#[test]
fn config_dir_resolution() -> Result<()> {
    // -- Case 1: XDG_CONFIG_HOME wins when set
    let env = env_map(&[("XDG_CONFIG_HOME", "/custom/config")]);
    assert_eq!(
        config_dir_with(&env).map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/custom/config")
    );

    // -- Case 2: falls back to $HOME/.config
    let env = env_map(&[("HOME", "/home/tester")]);
    assert_eq!(
        config_dir_with(&env).map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/home/tester").join(".config")
    );

    // -- Case 3: an empty XDG_CONFIG_HOME is ignored
    let env = env_map(&[("HOME", "/home/tester"), ("XDG_CONFIG_HOME", "")]);
    assert_eq!(
        config_dir_with(&env).map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/home/tester").join(".config")
    );

    // -- Case 4: a missing HOME fails
    let err = match config_dir_with(&EnvMap::new()) {
        Ok(_) => return Err("config_dir should fail when HOME missing".into()),
        Err(e) => e,
    };
    assert!(matches!(err, XdgError::HomeNotFound));
    Ok(())
}

#[test]
fn state_dir_resolution() -> Result<()> {
    // -- Case 1: XDG_STATE_HOME wins when set
    let env = env_map(&[("XDG_STATE_HOME", "/custom/state")]);
    assert_eq!(
        state_dir_with(&env).map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/custom/state")
    );

    // -- Case 2: falls back to $HOME/.local/state
    let env = env_map(&[("HOME", "/home/tester")]);
    assert_eq!(
        state_dir_with(&env).map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/home/tester").join(".local").join("state")
    );

    // -- Case 3: an empty XDG_STATE_HOME is ignored
    let env = env_map(&[("HOME", "/home/tester"), ("XDG_STATE_HOME", "")]);
    assert_eq!(
        state_dir_with(&env).map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/home/tester").join(".local").join("state")
    );

    // -- Case 4: a missing HOME fails
    let err = match state_dir_with(&EnvMap::new()) {
        Ok(_) => return Err("state_dir should fail when HOME missing".into()),
        Err(e) => e,
    };
    assert!(matches!(err, XdgError::HomeNotFound));
    Ok(())
}

#[test]
fn runtime_dir_resolution_and_error_display() -> Result<()> {
    // -- Case 1: XDG_RUNTIME_DIR wins when set
    let env = env_map(&[("XDG_RUNTIME_DIR", "/run/user/1000")]);
    assert_eq!(
        runtime_dir_with(&env).map_err(|e| format!("{e:?}"))?,
        PathBuf::from("/run/user/1000")
    );

    // -- Case 2: an unset XDG_RUNTIME_DIR fails
    let err = match runtime_dir_with(&EnvMap::new()) {
        Ok(_) => return Err("runtime_dir should fail when not set".into()),
        Err(e) => e,
    };
    assert!(matches!(err, XdgError::RuntimeDirNotSet));

    // -- Case 3: an empty XDG_RUNTIME_DIR fails
    let env = env_map(&[("XDG_RUNTIME_DIR", "")]);
    let err = match runtime_dir_with(&env) {
        Ok(_) => return Err("runtime_dir should fail when empty".into()),
        Err(e) => e,
    };
    assert!(matches!(err, XdgError::RuntimeDirNotSet));

    // -- Case 4: error display strings
    assert_eq!(
        format!("{}", XdgError::HomeNotFound),
        "HOME environment variable not set"
    );
    assert_eq!(
        format!("{}", XdgError::RuntimeDirNotSet),
        "XDG_RUNTIME_DIR not set and has no fallback"
    );
    Ok(())
}

// endregion: --- Tests
