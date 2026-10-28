//! Shared fixtures for the `Paths` resolution tests.

use std::path::{Path, PathBuf};

pub(crate) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The app directory name the current build must resolve.
#[cfg(debug_assertions)]
pub(crate) const EXPECTED_APP_DIR: &str = "nu-agent-dev";

/// The app directory name the current build must resolve.
#[cfg(not(debug_assertions))]
pub(crate) const EXPECTED_APP_DIR: &str = "nu-agent";

/// `{base}/{APP_DIR_NAME}` for the current build.
pub(crate) fn app_dir(base: &str) -> PathBuf {
    Path::new(base).join(EXPECTED_APP_DIR)
}
