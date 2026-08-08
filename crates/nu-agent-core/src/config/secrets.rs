use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::utils::xdg;

/// A general secret store for LLM provider API keys and OAuth tokens.
///
/// Persists to `$XDG_DATA_HOME/nu-agent/secrets.json` with 0600 permissions.
/// Referenced from config via `api_key = "store:openai"` instead of raw values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SecretStore {
    #[serde(skip)]
    pub path: PathBuf,
    pub secrets: HashMap<String, Credential>,
}

/// A single credential entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Credential {
    ApiKey {
        key: String,
    },
    OAuth {
        access_token: String,
        refresh_token: Option<String>,
        expires_at: Option<u64>,
    },
}

/// Errors that can occur during secret store operations.
#[derive(Debug)]
pub enum SecretStoreError {
    Io(std::io::Error),
    Parse(serde_json::Error),
    NoDataDir,
}

impl std::fmt::Display for SecretStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Parse(e) => write!(f, "failed to parse secrets.json: {e}"),
            Self::NoDataDir => write!(f, "no data directory found — set XDG_DATA_HOME"),
        }
    }
}

impl std::error::Error for SecretStoreError {}

impl SecretStore {
    /// Resolve the default file path.
    fn path() -> Result<PathBuf, SecretStoreError> {
        let dir = xdg::data_dir().map_err(|_| SecretStoreError::NoDataDir)?;
        Ok(dir.join("nu-agent").join("secrets.json"))
    }

    /// Load from the default path. Returns an empty store if the file does not exist.
    pub fn load() -> Result<Self, SecretStoreError> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self {
                path,
                secrets: HashMap::new(),
            });
        }
        let content = std::fs::read_to_string(&path).map_err(SecretStoreError::Io)?;
        let mut store: Self = serde_json::from_str(&content).map_err(SecretStoreError::Parse)?;
        store.path = path;
        Ok(store)
    }

    /// Save to the default path with 0600 permissions.
    pub fn save(&self) -> Result<(), SecretStoreError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(SecretStoreError::Io)?;
        }
        let content = serde_json::to_string_pretty(self).map_err(SecretStoreError::Parse)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&self.path)
                .map_err(SecretStoreError::Io)?;
            // Enforce 0600 even if the file already existed with looser permissions.
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(SecretStoreError::Io)?;
            let mut writer = std::io::BufWriter::new(&file);
            writer
                .write_all(content.as_bytes())
                .map_err(SecretStoreError::Io)?;
            writer.flush().map_err(SecretStoreError::Io)?;
        }

        #[cfg(not(unix))]
        {
            std::fs::write(&self.path, content).map_err(SecretStoreError::Io)?;
        }

        Ok(())
    }

    /// Get a credential by key.
    pub fn get(&self, key: &str) -> Option<&Credential> {
        self.secrets.get(key)
    }

    /// Insert or replace a credential.
    pub fn set(&mut self, key: String, credential: Credential) {
        self.secrets.insert(key, credential);
    }

    /// Remove a credential by key.
    pub fn remove(&mut self, key: &str) -> Option<Credential> {
        self.secrets.remove(key)
    }

    /// Resolve `store:openai` → lookup `openai` → return the API key string.
    pub fn resolve(&self, reference: &str) -> Option<String> {
        let key = reference.strip_prefix("store:")?;
        match self.secrets.get(key)? {
            Credential::ApiKey { key } => Some(key.clone()),
            Credential::OAuth { access_token, .. } => Some(access_token.clone()),
        }
    }

    /// List all keys with their credential type (for status display).
    pub fn list(&self) -> Vec<(&str, &Credential)> {
        self.secrets.iter().map(|(k, v)| (k.as_str(), v)).collect()
    }
}

#[cfg(test)]
#[path = "secrets_test.rs"]
mod secrets_test;
