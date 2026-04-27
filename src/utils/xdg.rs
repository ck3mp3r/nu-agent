//! XDG Base Directory Specification implementation
//!
//! This module provides proper XDG Base Directory support according to the spec:
//! https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html
//!
//! Each function checks the appropriate XDG_* environment variable first,
//! then falls back to the specified default path (except runtime_dir which has no fallback).

use std::env;
use std::path::PathBuf;

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
/// use nu_plugin_agent::utils::xdg;
///
/// let data_dir = xdg::data_dir().unwrap();
/// println!("Data directory: {:?}", data_dir);
/// ```
pub fn data_dir() -> Result<PathBuf, XdgError> {
    if let Ok(val) = env::var("XDG_DATA_HOME")
        && !val.is_empty()
    {
        return Ok(PathBuf::from(val));
    }
    let home = env::var("HOME").map_err(|_| XdgError::HomeNotFound)?;
    Ok(PathBuf::from(home).join(".local").join("share"))
}

/// Get the XDG cache directory
///
/// Checks XDG_CACHE_HOME first, falls back to ~/.cache
///
/// # Examples
///
/// ```
/// use nu_plugin_agent::utils::xdg;
///
/// let cache_dir = xdg::cache_dir().unwrap();
/// println!("Cache directory: {:?}", cache_dir);
/// ```
pub fn cache_dir() -> Result<PathBuf, XdgError> {
    if let Ok(val) = env::var("XDG_CACHE_HOME")
        && !val.is_empty()
    {
        return Ok(PathBuf::from(val));
    }
    let home = env::var("HOME").map_err(|_| XdgError::HomeNotFound)?;
    Ok(PathBuf::from(home).join(".cache"))
}

/// Get the XDG config directory
///
/// Checks XDG_CONFIG_HOME first, falls back to ~/.config
///
/// # Examples
///
/// ```
/// use nu_plugin_agent::utils::xdg;
///
/// let config_dir = xdg::config_dir().unwrap();
/// println!("Config directory: {:?}", config_dir);
/// ```
pub fn config_dir() -> Result<PathBuf, XdgError> {
    if let Ok(val) = env::var("XDG_CONFIG_HOME")
        && !val.is_empty()
    {
        return Ok(PathBuf::from(val));
    }
    let home = env::var("HOME").map_err(|_| XdgError::HomeNotFound)?;
    Ok(PathBuf::from(home).join(".config"))
}

/// Get the XDG state directory
///
/// Checks XDG_STATE_HOME first, falls back to ~/.local/state
///
/// # Examples
///
/// ```
/// use nu_plugin_agent::utils::xdg;
///
/// let state_dir = xdg::state_dir().unwrap();
/// println!("State directory: {:?}", state_dir);
/// ```
pub fn state_dir() -> Result<PathBuf, XdgError> {
    if let Ok(val) = env::var("XDG_STATE_HOME")
        && !val.is_empty()
    {
        return Ok(PathBuf::from(val));
    }
    let home = env::var("HOME").map_err(|_| XdgError::HomeNotFound)?;
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
/// use nu_plugin_agent::utils::xdg;
///
/// match xdg::runtime_dir() {
///     Ok(dir) => println!("Runtime directory: {:?}", dir),
///     Err(e) => eprintln!("Runtime directory not available: {}", e),
/// }
/// ```
pub fn runtime_dir() -> Result<PathBuf, XdgError> {
    env::var("XDG_RUNTIME_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .ok_or(XdgError::RuntimeDirNotSet)
}
