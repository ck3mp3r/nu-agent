use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fs2::FileExt;
use rmcp::transport::auth::{
    AuthError, CredentialStore, StateStore, StoredAuthorizationState, StoredCredentials,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// A single credential entry for one MCP server.
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct McpCredentialsEntry {
    pub server_url: Option<String>,
    /// rmcp's StoredCredentials (client_id + token_response + granted_scopes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stored_credentials: Option<StoredCredentials>,
}

impl std::fmt::Debug for McpCredentialsEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpCredentialsEntry")
            .field("server_url", &self.server_url)
            .field(
                "stored_credentials",
                &self.stored_credentials.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

/// File-backed credential store for MCP OAuth tokens and PKCE state.
///
/// Persists to `$XDG_DATA_HOME/nu-agent/mcp-auth.json` with 0600 permissions
/// and file locking via `fs2`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpCredentialsStore {
    #[serde(flatten)]
    pub entries: HashMap<String, McpCredentialsEntry>,
    /// OAuth authorization states keyed by CSRF token (for StateStore trait).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub oauth_states: HashMap<String, StoredAuthorizationState>,
}

/// Errors that can occur during credential store operations.
#[derive(Debug, thiserror::Error)]
pub enum CredentialsError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("No data directory found — set XDG_DATA_HOME")]
    NoDataDir,
}

impl McpCredentialsStore {
    /// Load from the default path (`$XDG_DATA_HOME/nu-agent/mcp-auth.json`).
    pub fn load() -> Result<Self, CredentialsError> {
        let path = Self::default_path()?;
        Self::load_from(&path)
    }

    /// Load from a specific path.
    ///
    /// Returns an empty store if the file does not exist or contains corrupt JSON.
    pub fn load_from(path: &Path) -> Result<Self, CredentialsError> {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let store: Self = serde_json::from_str(&content).unwrap_or_default();
                Ok(store)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(CredentialsError::Io(e)),
        }
    }

    /// Save to the default path with mode 0600 and file locking.
    pub fn save(&self) -> Result<(), CredentialsError> {
        let path = Self::default_path()?;
        self.save_to(&path)
    }

    /// Save to a specific path with mode 0600 and file locking.
    pub fn save_to(&self, path: &Path) -> Result<(), CredentialsError> {
        // Ensure parent dir exists
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

            // Enforce 0600 even if file already existed with looser permissions.
            // This must happen before writing so permissions are tightened even
            // if the write fails.
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;

            file.lock_exclusive()?;
            let mut writer = std::io::BufWriter::new(&file);
            writer.write_all(json.as_bytes())?;
            writer.flush()?;
            file.unlock()?;
        }

        #[cfg(not(unix))]
        {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(path)?;

            file.lock_exclusive()?;
            let mut writer = std::io::BufWriter::new(&file);
            writer.write_all(json.as_bytes())?;
            writer.flush()?;
            file.unlock()?;
        }

        Ok(())
    }

    /// Resolve the default file path.
    pub fn default_path() -> Result<PathBuf, CredentialsError> {
        let data_dir = crate::utils::xdg::data_dir().map_err(|_| CredentialsError::NoDataDir)?;
        Ok(data_dir.join("nu-agent").join("mcp-auth.json"))
    }

    /// Remove all credentials for a server.
    pub fn remove(&mut self, server_name: &str) {
        self.entries.remove(server_name);
    }
}

/// File-backed implementation of rmcp's [`CredentialStore`] trait.
///
/// Wraps an [`Arc<Mutex<McpCredentialsStore>>`] shared with [`FileStateStore`].
/// Each instance is scoped to a single MCP server name — the `server_name` is
/// provided at construction and used as the key for all operations.
#[derive(Debug, Clone)]
pub struct FileCredentialStore {
    store: Arc<Mutex<McpCredentialsStore>>,
    server_name: String,
    path: Option<PathBuf>,
}

impl FileCredentialStore {
    pub fn new(store: Arc<Mutex<McpCredentialsStore>>, server_name: impl Into<String>) -> Self {
        Self {
            store,
            server_name: server_name.into(),
            path: None,
        }
    }

    /// Create with a custom file path (for testing).
    pub fn new_with_path(
        store: Arc<Mutex<McpCredentialsStore>>,
        server_name: impl Into<String>,
        path: PathBuf,
    ) -> Self {
        Self {
            store,
            server_name: server_name.into(),
            path: Some(path),
        }
    }
}

#[async_trait::async_trait]
impl CredentialStore for FileCredentialStore {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        let guard = self.store.lock().await;
        let entry = guard.entries.get(&self.server_name);
        Ok(entry.and_then(|e| e.stored_credentials.clone()))
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        let mut guard = self.store.lock().await;
        let entry = guard.entries.entry(self.server_name.clone()).or_default();
        entry.stored_credentials = Some(credentials);
        match &self.path {
            Some(p) => guard
                .save_to(p)
                .map_err(|e| AuthError::InternalError(e.to_string())),
            None => guard
                .save()
                .map_err(|e| AuthError::InternalError(e.to_string())),
        }
    }

    async fn clear(&self) -> Result<(), AuthError> {
        let mut guard = self.store.lock().await;
        guard.entries.remove(&self.server_name);
        match &self.path {
            Some(p) => guard
                .save_to(p)
                .map_err(|e| AuthError::InternalError(e.to_string())),
            None => guard
                .save()
                .map_err(|e| AuthError::InternalError(e.to_string())),
        }
    }
}

/// File-backed implementation of rmcp's [`StateStore`] trait.
///
/// Wraps an [`Arc<Mutex<McpCredentialsStore>>`] shared with [`FileCredentialStore`].
/// OAuth authorization states are stored in the `oauth_states` map keyed by CSRF token.
#[derive(Debug, Clone)]
pub struct FileStateStore {
    store: Arc<Mutex<McpCredentialsStore>>,
    path: Option<PathBuf>,
}

impl FileStateStore {
    pub fn new(store: Arc<Mutex<McpCredentialsStore>>) -> Self {
        Self { store, path: None }
    }

    /// Create with a custom file path (for testing).
    pub fn new_with_path(store: Arc<Mutex<McpCredentialsStore>>, path: PathBuf) -> Self {
        Self {
            store,
            path: Some(path),
        }
    }
}

#[async_trait::async_trait]
impl StateStore for FileStateStore {
    async fn save(
        &self,
        csrf_token: &str,
        state: StoredAuthorizationState,
    ) -> Result<(), AuthError> {
        let mut guard = self.store.lock().await;
        guard.oauth_states.insert(csrf_token.to_string(), state);
        match &self.path {
            Some(p) => guard
                .save_to(p)
                .map_err(|e| AuthError::InternalError(e.to_string())),
            None => guard
                .save()
                .map_err(|e| AuthError::InternalError(e.to_string())),
        }
    }

    async fn load(&self, csrf_token: &str) -> Result<Option<StoredAuthorizationState>, AuthError> {
        let guard = self.store.lock().await;
        Ok(guard.oauth_states.get(csrf_token).cloned())
    }

    async fn delete(&self, csrf_token: &str) -> Result<(), AuthError> {
        let mut guard = self.store.lock().await;
        guard.oauth_states.remove(csrf_token);
        match &self.path {
            Some(p) => guard
                .save_to(p)
                .map_err(|e| AuthError::InternalError(e.to_string())),
            None => guard
                .save()
                .map_err(|e| AuthError::InternalError(e.to_string())),
        }
    }
}

#[cfg(test)]
#[path = "credentials_test.rs"]
mod credentials_test;
