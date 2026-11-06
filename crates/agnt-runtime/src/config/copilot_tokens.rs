//! GitHub Copilot credential persistence checks.
//!
//! rig 0.45's `Authenticator` keeps its record in a `TokenStore`; with
//! `FileTokenStore` that is `<token_dir>/copilot.json` (the store key is the
//! provider name). The login commands call
//! [`verify_copilot_login_persistence`] after the device flow returns, so a
//! silent no-op surfaces as an error instead of a success message. The status
//! and logout commands read the same record through
//! [`read_copilot_token_record`].

// region:    --- Modules

use std::path::{Path, PathBuf};

use serde::Deserialize;

// endregion: --- Modules

// region:    --- Constants

/// File name of rig's Copilot credential record inside the token directory.
///
/// `FileTokenStore` names each record `<key>.json`, and the Copilot
/// authenticator's key is the provider name, `copilot`.
const TOKEN_FILE: &str = "copilot.json";

/// Copilot credential directory name. Debug builds append `-dev`, so a debug
/// build never refreshes production GitHub Copilot credentials.
#[cfg(debug_assertions)]
const COPILOT_DIR_NAME: &str = "github_copilot-dev";

/// Copilot credential directory name. Debug builds append `-dev`, so a debug
/// build never refreshes production GitHub Copilot credentials.
#[cfg(not(debug_assertions))]
const COPILOT_DIR_NAME: &str = "github_copilot";

// endregion: --- Constants

// region:    --- Public Functions

/// The Copilot credential directory name: `github_copilot-dev` in debug
/// builds, `github_copilot` in release builds.
pub fn copilot_dir_name() -> &'static str {
    COPILOT_DIR_NAME
}

/// The platform default Copilot credential directory.
///
/// `$XDG_CONFIG_HOME/{COPILOT_DIR_NAME}` on unix (falling back to
/// `$HOME/.config/{COPILOT_DIR_NAME}`), `%APPDATA%\{COPILOT_DIR_NAME}` on
/// Windows. `COPILOT_DIR_NAME` is `github_copilot-dev` in debug builds and
/// `github_copilot` in release builds. `None` when no base directory can be
/// resolved.
pub fn copilot_token_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join(COPILOT_DIR_NAME))
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .map(|dir| dir.join(COPILOT_DIR_NAME))
    }
}

// region:    --- Types

/// The subset of rig's Copilot credential record read from `copilot.json`.
///
/// rig serializes `{ "token": ..., "expires_at": ... }`; unknown fields are
/// ignored.
#[derive(Deserialize)]
struct ApiKeyRecord {
    token: Option<String>,
    expires_at: Option<i64>,
}

/// A parsed Copilot API-key record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotTokenRecord {
    /// Whether the record holds a non-empty token.
    pub has_token: bool,
    /// The record's expiry in unix seconds, when present.
    pub expires_at: Option<i64>,
}

// endregion: --- Types

// region:    --- Public Functions

/// Verify that `<dir>/copilot.json` holds a non-empty Copilot token.
///
/// Returns `Err` when the file is missing, unparsable, or the token is empty.
/// The error message names the resolved path so the storage location is visible.
pub fn verify_copilot_login_persistence(dir: &Path) -> Result<(), String> {
    let path = dir.join(TOKEN_FILE);
    let bytes = std::fs::read(&path).map_err(|e| {
        format!(
            "GitHub Copilot credentials were not persisted: {} is unreadable: {e}",
            path.display()
        )
    })?;
    let record: ApiKeyRecord = serde_json::from_slice(&bytes).map_err(|e| {
        format!(
            "GitHub Copilot credentials were not persisted: {} is not valid JSON: {e}",
            path.display()
        )
    })?;
    if record.token.unwrap_or_default().trim().is_empty() {
        return Err(format!(
            "GitHub Copilot credentials were not persisted: {} holds no token",
            path.display()
        ));
    }
    Ok(())
}

/// Read and parse `<dir>/copilot.json`.
///
/// Returns `Ok(None)` when the file does not exist, `Ok(Some(record))` when it
/// parses, and `Err` when it exists but is unreadable or unparsable.
pub fn read_copilot_token_record(dir: &Path) -> Result<Option<CopilotTokenRecord>, String> {
    let path = dir.join(TOKEN_FILE);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(format!(
                "GitHub Copilot credential file {} is unreadable: {e}",
                path.display()
            ));
        }
    };
    let record: ApiKeyRecord = serde_json::from_slice(&bytes).map_err(|e| {
        format!(
            "GitHub Copilot credential file {} is not valid JSON: {e}",
            path.display()
        )
    })?;
    Ok(Some(CopilotTokenRecord {
        has_token: !record.token.unwrap_or_default().trim().is_empty(),
        expires_at: record.expires_at,
    }))
}

// endregion: --- Public Functions

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/config/copilot_tokens.rs"]
mod copilot_tokens_test;

// endregion: --- Tests
