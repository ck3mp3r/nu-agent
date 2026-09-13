use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The persisted TUI theme preference.
///
/// Stored as a single JSON key `{"theme": "<kebab-name>"}` at
/// `$XDG_DATA_HOME/nu-agent/theme.json` with mode 0600, following the
/// `McpCredentialsStore` file pattern.
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
    #[error("No data directory found — set XDG_DATA_HOME")]
    NoDataDir,
}

impl ThemePreference {
    /// Load from the default path (`$XDG_DATA_HOME/nu-agent/theme.json`).
    pub fn load() -> Result<Self, ThemePreferenceError> {
        let path = Self::default_path()?;
        Self::load_from(&path)
    }

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

    /// Save to the default path with mode 0600.
    pub fn save(&self) -> Result<(), ThemePreferenceError> {
        let path = Self::default_path()?;
        self.save_to(&path)
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

    /// Resolve the default file path.
    pub fn default_path() -> Result<PathBuf, ThemePreferenceError> {
        let data_dir =
            crate::utils::xdg::data_dir().map_err(|_| ThemePreferenceError::NoDataDir)?;
        Ok(data_dir.join("nu-agent").join("theme.json"))
    }
}

#[cfg(test)]
#[path = "theme_pref_test.rs"]
mod theme_pref_test;
