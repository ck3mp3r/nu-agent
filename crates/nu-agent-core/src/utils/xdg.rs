//! XDG Base Directory Specification implementation
//!
//! This module provides proper XDG Base Directory support according to the spec:
//! <https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html>
//!
//! Each function checks the appropriate XDG_* environment variable first,
//! then falls back to the specified default path (except runtime_dir which has no fallback).
//!
//! Every resolver has a `*_with` variant taking an explicit [`EnvMap`]. The
//! bare functions read the real process environment, so production callers keep
//! their existing call sites while tests inject a map and run in parallel.

use std::path::PathBuf;

use super::env_map::{EnvMap, process_env};

/// Errors that can occur when resolving XDG directories
#[derive(Debug)]
pub enum XdgError {
    /// The HOME environment variable is not set or is invalid
    HomeNotFound,
    /// XDG_RUNTIME_DIR is not set (no fallback per spec)
    RuntimeDirNotSet,
}

impl std::fmt::Display for XdgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XdgError::HomeNotFound => write!(f, "HOME environment variable not set"),
            XdgError::RuntimeDirNotSet => write!(f, "XDG_RUNTIME_DIR not set and has no fallback"),
        }
    }
}

impl std::error::Error for XdgError {}

/// Get the XDG data directory
///
/// Checks XDG_DATA_HOME first, falls back to ~/.local/share
///
/// # Examples
///
/// ```
/// use nu_agent_core::utils::xdg;
///
/// let data_dir = xdg::data_dir().unwrap();
/// println!("Data directory: {:?}", data_dir);
/// ```
pub fn data_dir() -> Result<PathBuf, XdgError> {
    data_dir_with(&process_env())
}

/// Get the XDG data directory from an explicit environment map.
pub fn data_dir_with(env: &EnvMap) -> Result<PathBuf, XdgError> {
    if let Some(val) = non_empty(env, "XDG_DATA_HOME") {
        return Ok(PathBuf::from(val));
    }
    let home = home(env)?;
    Ok(PathBuf::from(home).join(".local").join("share"))
}

/// Get the XDG cache directory
///
/// Checks XDG_CACHE_HOME first, falls back to ~/.cache
///
/// # Examples
///
/// ```
/// use nu_agent_core::utils::xdg;
///
/// let cache_dir = xdg::cache_dir().unwrap();
/// println!("Cache directory: {:?}", cache_dir);
/// ```
pub fn cache_dir() -> Result<PathBuf, XdgError> {
    cache_dir_with(&process_env())
}

/// Get the XDG cache directory from an explicit environment map.
pub fn cache_dir_with(env: &EnvMap) -> Result<PathBuf, XdgError> {
    if let Some(val) = non_empty(env, "XDG_CACHE_HOME") {
        return Ok(PathBuf::from(val));
    }
    let home = home(env)?;
    Ok(PathBuf::from(home).join(".cache"))
}

/// Get the XDG config directory
///
/// Checks XDG_CONFIG_HOME first, falls back to ~/.config
///
/// # Examples
///
/// ```
/// use nu_agent_core::utils::xdg;
///
/// let config_dir = xdg::config_dir().unwrap();
/// println!("Config directory: {:?}", config_dir);
/// ```
pub fn config_dir() -> Result<PathBuf, XdgError> {
    config_dir_with(&process_env())
}

/// Get the XDG config directory from an explicit environment map.
pub fn config_dir_with(env: &EnvMap) -> Result<PathBuf, XdgError> {
    if let Some(val) = non_empty(env, "XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(val));
    }
    let home = home(env)?;
    Ok(PathBuf::from(home).join(".config"))
}

/// Get the XDG state directory
///
/// Checks XDG_STATE_HOME first, falls back to ~/.local/state
///
/// # Examples
///
/// ```
/// use nu_agent_core::utils::xdg;
///
/// let state_dir = xdg::state_dir().unwrap();
/// println!("State directory: {:?}", state_dir);
/// ```
pub fn state_dir() -> Result<PathBuf, XdgError> {
    state_dir_with(&process_env())
}

/// Get the XDG state directory from an explicit environment map.
pub fn state_dir_with(env: &EnvMap) -> Result<PathBuf, XdgError> {
    if let Some(val) = non_empty(env, "XDG_STATE_HOME") {
        return Ok(PathBuf::from(val));
    }
    let home = home(env)?;
    Ok(PathBuf::from(home).join(".local").join("state"))
}

/// Get the XDG runtime directory
///
/// Checks XDG_RUNTIME_DIR - NO FALLBACK per XDG spec
///
/// The XDG Base Directory spec requires that XDG_RUNTIME_DIR has no fallback.
/// If it's not set, applications should fail rather than create insecure alternatives.
///
/// # Examples
///
/// ```
/// use nu_agent_core::utils::xdg;
///
/// match xdg::runtime_dir() {
///     Ok(dir) => println!("Runtime directory: {:?}", dir),
///     Err(e) => eprintln!("Runtime directory not available: {}", e),
/// }
/// ```
pub fn runtime_dir() -> Result<PathBuf, XdgError> {
    runtime_dir_with(&process_env())
}

/// Get the XDG runtime directory from an explicit environment map.
pub fn runtime_dir_with(env: &EnvMap) -> Result<PathBuf, XdgError> {
    non_empty(env, "XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or(XdgError::RuntimeDirNotSet)
}

/// Look up `key`, treating an empty value as absent.
fn non_empty<'a>(env: &'a EnvMap, key: &str) -> Option<&'a str> {
    env.get(key).map(String::as_str).filter(|s| !s.is_empty())
}

/// Look up HOME, reporting [`XdgError::HomeNotFound`] when absent or empty.
fn home(env: &EnvMap) -> Result<&str, XdgError> {
    non_empty(env, "HOME").ok_or(XdgError::HomeNotFound)
}
