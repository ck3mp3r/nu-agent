use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The persisted TUI theme preference.
///
/// Stored as a single JSON key `{"theme": "<kebab-name>"}` at the resolved
/// [`Paths::theme_path`](crate::config::Paths::theme_path) with mode 0600.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThemePreference {
    /// The kebab-case theme name (e.g. "catppuccin-frappe").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
}

/// Errors that can occur during theme preference operations.
#[derive(Debug, thiserror::Error)]
pub enum ThemePreferenceError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl ThemePreference {
    /// Load from a specific path.
    ///
    /// Returns an empty preference if the file does not exist or contains
    /// corrupt JSON.
    pub fn load_from(path: &Path) -> Result<Self, ThemePreferenceError> {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let pref: Self = serde_json::from_str(&content).unwrap_or_default();
                Ok(pref)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(ThemePreferenceError::Io(e)),
        }
    }

    /// Save to a specific path with mode 0600.
    pub fn save_to(&self, path: &Path) -> Result<(), ThemePreferenceError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(self)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(path)?;

            // Enforce 0600 even if the file already existed with looser
            // permissions.
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;

            let mut writer = std::io::BufWriter::new(&file);
            writer.write_all(json.as_bytes())?;
            writer.flush()?;
        }

        #[cfg(not(unix))]
        {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(path)?;

            let mut writer = std::io::BufWriter::new(&file);
            writer.write_all(json.as_bytes())?;
            writer.flush()?;
        }

        Ok(())
    }
}

#[cfg(test)]
#[path = "../test/theme_pref.rs"]
mod theme_pref_test;
