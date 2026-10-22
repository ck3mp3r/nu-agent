//! Application storage directory names.
//!
//! The directory name flips at compile time: debug builds resolve to
//! `nu-agent-dev`, release builds to `nu-agent`. A debug binary therefore
//! cannot open the production session database, secrets file, audit log, or
//! keychain entries.
//!
//! Every consumer derives its path from [`app_dir`] or [`data_app_dir`], so the
//! name lives in exactly one place.

// region:    --- Modules

use std::path::{Path, PathBuf};

use super::env_map::{EnvMap, process_env};
use super::xdg::{self, XdgError};

// endregion: --- Modules

// region:    --- Constants

/// Application directory name. Debug builds append `-dev`.
#[cfg(debug_assertions)]
pub const APP_DIR_NAME: &str = "nu-agent-dev";

/// Application directory name. Debug builds append `-dev`.
#[cfg(not(debug_assertions))]
pub const APP_DIR_NAME: &str = "nu-agent";

// endregion: --- Constants

// region:    --- Public Functions

/// Join the application directory name onto `base`.
pub fn app_dir(base: impl AsRef<Path>) -> PathBuf {
    base.as_ref().join(APP_DIR_NAME)
}

/// Resolve `{XDG_DATA_HOME or ~/.local/share}/{APP_DIR_NAME}`.
pub fn data_app_dir() -> Result<PathBuf, XdgError> {
    data_app_dir_with(&process_env())
}

/// Resolve `{XDG_DATA_HOME or ~/.local/share}/{APP_DIR_NAME}` from an explicit
/// environment map.
pub fn data_app_dir_with(env: &EnvMap) -> Result<PathBuf, XdgError> {
    Ok(app_dir(xdg::data_dir_with(env)?))
}

// endregion: --- Public Functions

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/utils/paths.rs"]
mod paths_test;

// endregion: --- Tests
