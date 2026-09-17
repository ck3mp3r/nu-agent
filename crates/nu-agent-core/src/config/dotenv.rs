//! `.env` file loader.
//!
//! Populates `std::env` from two `.env` files before config resolution, so
//! `Config::from_env()` can read the values. A variable already present in the
//! process environment is never overwritten — an inline env var such as
//! `OPENAI_API_KEY=sk-temp agent run` always wins over a `.env` file.

// region:    --- Modules

use std::path::{Path, PathBuf};

// endregion: --- Modules

// region:    --- Constants

/// Subdirectory of the XDG config directory holding the user-level `.env`.
const CONFIG_SUBDIR: &str = "nu-agent";

/// File name of the `.env` file.
const DOTENV_FILE: &str = ".env";

// endregion: --- Constants

// region:    --- Public Functions

/// Load `.env` files into the process environment.
///
/// Loads the user-level file first, then the project-level `./.env`, so a
/// project value overrides a user-level value for keys not already set in the
/// process environment. Missing files are ignored. Malformed lines are logged
/// and skipped — this function never fails.
pub fn load_env_files() {
    if let Ok(path) = user_env_path() {
        load_env_file(&path);
    }
    load_env_file(Path::new(DOTENV_FILE));
}

// endregion: --- Public Functions

// region:    --- Support

/// Resolve `$XDG_CONFIG_HOME/nu-agent/.env`.
fn user_env_path() -> Result<PathBuf, crate::utils::xdg::XdgError> {
    Ok(crate::utils::xdg::config_dir()?
        .join(CONFIG_SUBDIR)
        .join(DOTENV_FILE))
}

/// Parse one `.env` file and populate unset env vars from it.
fn load_env_file(path: &Path) {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        // A missing file is not an error — there is simply nothing to load.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            log::warn!("Failed to read {}: {e}", path.display());
            return;
        }
    };

    for (index, raw_line) in content.lines().enumerate() {
        let line_number = index + 1;
        match parse_line(raw_line) {
            Ok(Some((key, value))) => set_if_unset(&key, &value),
            Ok(None) => {}
            Err(reason) => {
                log::warn!(
                    "Ignoring malformed line {line_number} in {}: {reason}",
                    path.display()
                );
            }
        }
    }
}

/// Parse a single `.env` line.
///
/// Returns `Ok(None)` for blank lines and comments, `Ok(Some((key, value)))`
/// for an assignment, and `Err(reason)` for a malformed line.
fn parse_line(raw_line: &str) -> Result<Option<(String, String)>, String> {
    let line = raw_line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }

    // Strip an optional `export ` prefix.
    let assignment = line.strip_prefix("export ").unwrap_or(line).trim_start();

    let Some((raw_key, raw_value)) = assignment.split_once('=') else {
        return Err("expected KEY=value".to_string());
    };

    let key = raw_key.trim();
    if key.is_empty() {
        return Err("empty key".to_string());
    }

    Ok(Some((key.to_string(), strip_quotes(raw_value.trim()))))
}

/// Remove one layer of matching single or double quotes.
fn strip_quotes(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && let (Some(first), Some(last)) = (bytes.first(), bytes.last())
        && first == last
        && (*first == b'\'' || *first == b'"')
    {
        return value[1..value.len() - 1].to_string();
    }
    value.to_string()
}

/// Set an env var only when it is not already present.
fn set_if_unset(key: &str, value: &str) {
    if std::env::var(key).is_ok() {
        return;
    }
    // SAFETY: called once at startup, before the process spawns threads that
    // read the environment concurrently.
    unsafe {
        std::env::set_var(key, value);
    }
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "dotenv_test.rs"]
mod dotenv_test;

// endregion: --- Tests
