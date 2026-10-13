//! GitHub Copilot credential persistence checks.
//!
//! rig writes the exchanged Copilot API key to `<token_dir>/api-key.json` only
//! when the api-key path is configured; a `None` path makes the write a no-op.
//! The login commands call [`verify_copilot_login_persistence`] after
//! `authenticate` returns, so a silent no-op surfaces as an error instead of a
//! success message. The status and logout commands read the same record through
//! [`read_copilot_token_record`].

// region:    --- Modules

use std::path::Path;

use serde::Deserialize;

// endregion: --- Modules

// region:    --- Constants

/// File name of rig's Copilot API-key record inside the token directory.
const API_KEY_FILE: &str = "api-key.json";

// endregion: --- Constants

// region:    --- Types

/// The subset of rig's `ApiKeyRecord` read from `api-key.json`.
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

/// Verify that `<dir>/api-key.json` holds a non-empty Copilot token.
///
/// Returns `Err` when the file is missing, unparsable, or the token is empty.
/// The error message names the resolved path so the storage location is visible.
pub fn verify_copilot_login_persistence(dir: &Path) -> Result<(), String> {
    let path = dir.join(API_KEY_FILE);
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

/// Read and parse `<dir>/api-key.json`.
///
/// Returns `Ok(None)` when the file does not exist, `Ok(Some(record))` when it
/// parses, and `Err` when it exists but is unreadable or unparsable.
pub fn read_copilot_token_record(dir: &Path) -> Result<Option<CopilotTokenRecord>, String> {
    let path = dir.join(API_KEY_FILE);
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
