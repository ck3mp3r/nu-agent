//! Tests for XDG Base Directory implementation
//!
//! These tests use serial_test to avoid race conditions when manipulating environment variables.

use crate::utils::xdg::*;
use serial_test::serial;
use std::env;
use std::path::PathBuf;

// Helper to clean up environment variables after tests
fn cleanup_env() {
    // SAFETY: These tests use serial_test to ensure exclusive access
    // to the environment, so no other threads can observe inconsistent state
    unsafe {
        env::remove_var("XDG_DATA_HOME");
        env::remove_var("XDG_CACHE_HOME");
        env::remove_var("XDG_CONFIG_HOME");
        env::remove_var("XDG_STATE_HOME");
        env::remove_var("XDG_RUNTIME_DIR");
    }
}

// Helper to set environment variable safely in tests
fn set_env(key: &str, value: &str) {
    // SAFETY: These tests use serial_test to ensure exclusive access
    // to the environment, so no other threads can observe inconsistent state
    unsafe {
        env::set_var(key, value);
    }
}

// Helper to remove environment variable safely in tests
fn remove_env(key: &str) {
    // SAFETY: These tests use serial_test to ensure exclusive access
    // to the environment, so no other threads can observe inconsistent state
    unsafe {
        env::remove_var(key);
    }
}

#[test]
#[serial]
fn data_dir_uses_xdg_data_home_when_set() {
    cleanup_env();
    set_env("XDG_DATA_HOME", "/custom/data");
    assert_eq!(data_dir().unwrap(), PathBuf::from("/custom/data"));
    cleanup_env();
}

#[test]
#[serial]
fn data_dir_falls_back_to_home_local_share() {
    cleanup_env();
    remove_env("XDG_DATA_HOME");
    let home = env::var("HOME").expect("HOME should be set for tests");
    let expected = PathBuf::from(home).join(".local").join("share");
    assert_eq!(data_dir().unwrap(), expected);
    cleanup_env();
}

#[test]
#[serial]
fn data_dir_ignores_empty_xdg_data_home() {
    cleanup_env();
    set_env("XDG_DATA_HOME", "");
    let home = env::var("HOME").expect("HOME should be set for tests");
    let expected = PathBuf::from(home).join(".local").join("share");
    assert_eq!(data_dir().unwrap(), expected);
    cleanup_env();
}

#[test]
#[serial]
fn data_dir_fails_when_home_missing() {
    cleanup_env();
    let home = env::var("HOME").ok();
    remove_env("HOME");
    remove_env("XDG_DATA_HOME");

    let result = data_dir();
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), XdgError::HomeNotFound));

    // Restore HOME
    if let Some(h) = home {
        set_env("HOME", &h);
    }
    cleanup_env();
}

#[test]
#[serial]
fn cache_dir_uses_xdg_cache_home_when_set() {
    cleanup_env();
    set_env("XDG_CACHE_HOME", "/custom/cache");
    assert_eq!(cache_dir().unwrap(), PathBuf::from("/custom/cache"));
    cleanup_env();
}

#[test]
#[serial]
fn cache_dir_falls_back_to_home_cache() {
    cleanup_env();
    remove_env("XDG_CACHE_HOME");
    let home = env::var("HOME").expect("HOME should be set for tests");
    let expected = PathBuf::from(home).join(".cache");
    assert_eq!(cache_dir().unwrap(), expected);
    cleanup_env();
}

#[test]
#[serial]
fn cache_dir_ignores_empty_xdg_cache_home() {
    cleanup_env();
    set_env("XDG_CACHE_HOME", "");
    let home = env::var("HOME").expect("HOME should be set for tests");
    let expected = PathBuf::from(home).join(".cache");
    assert_eq!(cache_dir().unwrap(), expected);
    cleanup_env();
}

#[test]
#[serial]
fn cache_dir_fails_when_home_missing() {
    cleanup_env();
    let home = env::var("HOME").ok();
    remove_env("HOME");
    remove_env("XDG_CACHE_HOME");

    let result = cache_dir();
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), XdgError::HomeNotFound));

    // Restore HOME
    if let Some(h) = home {
        set_env("HOME", &h);
    }
    cleanup_env();
}

#[test]
#[serial]
fn config_dir_uses_xdg_config_home_when_set() {
    cleanup_env();
    set_env("XDG_CONFIG_HOME", "/custom/config");
    assert_eq!(config_dir().unwrap(), PathBuf::from("/custom/config"));
    cleanup_env();
}

#[test]
#[serial]
fn config_dir_falls_back_to_home_config() {
    cleanup_env();
    remove_env("XDG_CONFIG_HOME");
    let home = env::var("HOME").expect("HOME should be set for tests");
    let expected = PathBuf::from(home).join(".config");
    assert_eq!(config_dir().unwrap(), expected);
    cleanup_env();
}

#[test]
#[serial]
fn config_dir_ignores_empty_xdg_config_home() {
    cleanup_env();
    set_env("XDG_CONFIG_HOME", "");
    let home = env::var("HOME").expect("HOME should be set for tests");
    let expected = PathBuf::from(home).join(".config");
    assert_eq!(config_dir().unwrap(), expected);
    cleanup_env();
}

#[test]
#[serial]
fn config_dir_fails_when_home_missing() {
    cleanup_env();
    let home = env::var("HOME").ok();
    remove_env("HOME");
    remove_env("XDG_CONFIG_HOME");

    let result = config_dir();
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), XdgError::HomeNotFound));

    // Restore HOME
    if let Some(h) = home {
        set_env("HOME", &h);
    }
    cleanup_env();
}

#[test]
#[serial]
fn state_dir_uses_xdg_state_home_when_set() {
    cleanup_env();
    set_env("XDG_STATE_HOME", "/custom/state");
    assert_eq!(state_dir().unwrap(), PathBuf::from("/custom/state"));
    cleanup_env();
}

#[test]
#[serial]
fn state_dir_falls_back_to_home_local_state() {
    cleanup_env();
    remove_env("XDG_STATE_HOME");
    let home = env::var("HOME").expect("HOME should be set for tests");
    let expected = PathBuf::from(home).join(".local").join("state");
    assert_eq!(state_dir().unwrap(), expected);
    cleanup_env();
}

#[test]
#[serial]
fn state_dir_ignores_empty_xdg_state_home() {
    cleanup_env();
    set_env("XDG_STATE_HOME", "");
    let home = env::var("HOME").expect("HOME should be set for tests");
    let expected = PathBuf::from(home).join(".local").join("state");
    assert_eq!(state_dir().unwrap(), expected);
    cleanup_env();
}

#[test]
#[serial]
fn state_dir_fails_when_home_missing() {
    cleanup_env();
    let home = env::var("HOME").ok();
    remove_env("HOME");
    remove_env("XDG_STATE_HOME");

    let result = state_dir();
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), XdgError::HomeNotFound));

    // Restore HOME
    if let Some(h) = home {
        set_env("HOME", &h);
    }
    cleanup_env();
}

#[test]
#[serial]
fn runtime_dir_uses_xdg_runtime_dir_when_set() {
    cleanup_env();
    set_env("XDG_RUNTIME_DIR", "/run/user/1000");
    assert_eq!(runtime_dir().unwrap(), PathBuf::from("/run/user/1000"));
    cleanup_env();
}

#[test]
#[serial]
fn runtime_dir_fails_when_not_set() {
    cleanup_env();
    remove_env("XDG_RUNTIME_DIR");

    let result = runtime_dir();
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), XdgError::RuntimeDirNotSet));

    cleanup_env();
}

#[test]
#[serial]
fn runtime_dir_fails_when_empty() {
    cleanup_env();
    set_env("XDG_RUNTIME_DIR", "");

    let result = runtime_dir();
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), XdgError::RuntimeDirNotSet));

    cleanup_env();
}

#[test]
#[serial]
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
