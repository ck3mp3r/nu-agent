//! `.env` file loader.
//!
//! Populates `std::env` from two `.env` files before config resolution, so
//! `Config::from_env()` can read the values. A variable already present in the
//! process environment is never overwritten — an inline env var such as
//! `OPENAI_API_KEY=sk-temp agent run` always wins over a `.env` file.
//!
//! The parsing core works on an explicit [`EnvMap`], so tests inject a map and
//! never touch process-global state.

// region:    --- Modules

use std::path::Path;

use crate::utils::env_map::{EnvMap, process_env};

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
    let mut env = process_env();
    let user_config_dir = crate::utils::xdg::config_dir().ok();
    let applied = load_env_files_into(&mut env, user_config_dir.as_deref(), Path::new("."));
    for (key, value) in applied {
        // SAFETY: called once at startup, before the process spawns threads that
        // read the environment concurrently.
        unsafe {
            std::env::set_var(key, value);
        }
    }
}

/// Merge the user-level and project-level `.env` files into `env`.
///
/// `user_config_dir` is the XDG config directory, or `None` when it cannot be
/// resolved. `project_dir` holds the project-level `.env`. Entries already
/// present in `env` are never overwritten. Returns the assignments that were
/// applied, in load order.
pub fn load_env_files_into(
    env: &mut EnvMap,
    user_config_dir: Option<&Path>,
    project_dir: &Path,
) -> Vec<(String, String)> {
    let mut applied = Vec::new();
    if let Some(dir) = user_config_dir {
        applied.extend(load_env_file_into(
            env,
            &dir.join(CONFIG_SUBDIR).join(DOTENV_FILE),
        ));
    }
    applied.extend(load_env_file_into(env, &project_dir.join(DOTENV_FILE)));
    applied
}

// endregion: --- Public Functions

// region:    --- Support

/// Parse one `.env` file and fill unset entries in `env`.
///
/// Returns the assignments that were applied. A missing file is not an error —
/// there is simply nothing to load.
fn load_env_file_into(env: &mut EnvMap, path: &Path) -> Vec<(String, String)> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            log::warn!("Failed to read {}: {e}", path.display());
            return Vec::new();
        }
    };

    let mut applied = Vec::new();
    for (index, raw_line) in content.lines().enumerate() {
        let line_number = index + 1;
        match parse_line(raw_line) {
            Ok(Some((key, value))) => {
                if env.contains_key(&key) {
                    continue;
                }
                env.insert(key.clone(), value.clone());
                applied.push((key, value));
            }
            Ok(None) => {}
            Err(reason) => {
                log::warn!(
                    "Ignoring malformed line {line_number} in {}: {reason}",
                    path.display()
                );
            }
        }
    }
    applied
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

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/config/dotenv.rs"]
mod dotenv_test;

// endregion: --- Tests
