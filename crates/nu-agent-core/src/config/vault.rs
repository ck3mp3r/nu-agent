//! Vault — the single interface for secret storage and retrieval.
//!
//! Every consumer (provider auth commands, MCP auth commands, the config
//! resolver, and the MCP runtime) interacts with secrets through [`Vault`].
//! The storage backend is selected once at startup by [`Vault::auto_detect`]
//! and dispatched statically through [`VaultBackendKind`] — no `Box<dyn>` and
//! no vtable.

// region:    --- Modules

use std::sync::Arc;

use super::file_backend::FileBackend;
use super::keychain_backend::KeychainBackend;
use rmcp::transport::auth::{
    AuthError, CredentialStore, StateStore, StoredAuthorizationState, StoredCredentials,
};
use serde::{Deserialize, Serialize};

// endregion: --- Modules

// region:    --- Constants

/// Prefix marking a value as a vault reference rather than a literal secret.
const STORE_PREFIX: &str = "store:";

/// Key prefix for LLM provider credentials.
const PROVIDER_PREFIX: &str = "provider:";

/// Key prefix for MCP OAuth credentials.
const MCP_CRED_PREFIX: &str = "mcp:cred:";

/// Key prefix for MCP OAuth PKCE/CSRF state.
const MCP_STATE_PREFIX: &str = "mcp:state:";

/// Key prefix for MCP bearer tokens.
const MCP_BEARER_PREFIX: &str = "mcp:bearer:";

// endregion: --- Constants

// region:    --- Types

// -- Vault

/// The single point of interaction with secret storage and retrieval.
///
/// Owns a [`VaultBackendKind`] and exposes typed methods for each secret
/// category. Consumers never touch a backend directly.
#[derive(Debug)]
pub struct Vault {
    backend: VaultBackendKind,
}

/// Two vaults are equal when they use the same backend variant.
///
/// The comparison never inspects stored values — only the backend kind — so it
/// is safe to derive `PartialEq` for configuration comparison.
impl PartialEq for Vault {
    fn eq(&self, other: &Self) -> bool {
        self.backend.kind() == other.backend.kind()
    }
}

// -- VaultBackendKind

/// Runtime backend selector.
///
/// A sum type that dispatches to one of three backends. It implements
/// [`VaultBackend`] through the match arms below — static, monomorphized
/// dispatch with no trait object.
#[derive(Debug)]
pub enum VaultBackendKind {
    Keychain(KeychainBackend),
    File(FileBackend),
    EnvOnly(EnvOnlyBackend),
}

impl VaultBackendKind {
    /// Identifies the backend variant without exposing the backend internals.
    pub fn kind(&self) -> BackendKind {
        match self {
            Self::Keychain(_) => BackendKind::Keychain,
            Self::File(_) => BackendKind::File,
            Self::EnvOnly(_) => BackendKind::EnvOnly,
        }
    }
}

/// Which backend variant a [`VaultBackendKind`] holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Keychain,
    File,
    EnvOnly,
}

// -- VaultBackend

/// Interface contract implemented by every storage backend.
///
/// The trait is internal to this module — consumers only ever see [`Vault`].
pub trait VaultBackend: Send + Sync {
    /// Return the stored value, or `None` when the key is absent.
    fn get(&self, key: &str) -> Result<Option<String>, VaultError>;

    /// Store a value under a key.
    fn set(&self, key: &str, value: &str) -> Result<(), VaultError>;

    /// Delete a key. Deleting an absent key is not an error.
    fn delete(&self, key: &str) -> Result<(), VaultError>;

    /// List every stored key.
    fn list(&self) -> Result<Vec<String>, VaultError>;
}

// -- VaultError

/// Errors that can occur during vault operations.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Keychain error: {0}")]
    Keychain(String),
    #[error("No vault backend available")]
    NoBackend,
    #[error("No data directory found — set XDG_DATA_HOME")]
    NoDataDir,
    #[error("Key not found in vault: {0}")]
    KeyNotFound(String),
}

// -- Credential

/// A single credential entry, serialized into a backend value.
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

// -- ProviderEntry

/// A provider credential summary, for status display.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderEntry {
    pub name: String,
    pub credential_type: CredentialType,
    pub expires_at: Option<u64>,
}

/// The kind of credential held for a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialType {
    ApiKey,
    OAuth,
}

// endregion: --- Constants

// region:    --- Types

// region:    --- Backends

/// Environment-only fallback backend.
///
/// Stores nothing: `get` always returns `None`, `set` and `delete` do nothing,
/// and `list` returns empty. It is the unconditional final fallback of
/// [`Vault::auto_detect`], for pure Docker/CI environments where env vars are
/// the only secret source.
#[derive(Debug)]
pub struct EnvOnlyBackend;

impl VaultBackend for EnvOnlyBackend {
    fn get(&self, _key: &str) -> Result<Option<String>, VaultError> {
        Ok(None)
    }

    fn set(&self, _key: &str, _value: &str) -> Result<(), VaultError> {
        Ok(())
    }

    fn delete(&self, _key: &str) -> Result<(), VaultError> {
        Ok(())
    }

    fn list(&self) -> Result<Vec<String>, VaultError> {
        Ok(Vec::new())
    }
}

// endregion: --- Backends

// region:    --- Vault Impl

impl Vault {
    // --- Construction ---

    /// Probe backends in priority order: keychain, then file, then env-only.
    /// The first available backend wins. Every probe is now real.
    pub fn auto_detect() -> Self {
        if let Ok(kc) = KeychainBackend::probe() {
            return Self {
                backend: VaultBackendKind::Keychain(kc),
            };
        }
        if let Ok(fb) = FileBackend::probe() {
            return Self {
                backend: VaultBackendKind::File(fb),
            };
        }
        Self {
            backend: VaultBackendKind::EnvOnly(EnvOnlyBackend),
        }
    }

    /// Construct a vault with a specific backend.
    pub fn new(backend: VaultBackendKind) -> Self {
        Self { backend }
    }

    /// Construct an env-only vault. All lookups return `None`.
    pub fn env_only() -> Self {
        Self {
            backend: VaultBackendKind::EnvOnly(EnvOnlyBackend),
        }
    }

    // --- Accessors ---

    /// Borrow the selected backend.
    pub fn backend(&self) -> &VaultBackendKind {
        &self.backend
    }

    // --- Provider Credentials ---

    /// Resolve `store:openai` to the stored key. Returns `None` when the
    /// reference is not a `store:` reference or the key is absent.
    pub fn resolve(&self, reference: &str) -> Option<String> {
        let name = reference.strip_prefix(STORE_PREFIX)?;
        let raw = self.backend.get(&provider_key(name)).ok()??;
        let credential: Credential = serde_json::from_str(&raw).ok()?;
        Some(match credential {
            Credential::ApiKey { key } => key,
            Credential::OAuth { access_token, .. } => access_token,
        })
    }

    /// Store a provider API key.
    pub fn store_api_key(&self, name: &str, key: &str) -> Result<(), VaultError> {
        let credential = Credential::ApiKey {
            key: key.to_string(),
        };
        self.backend
            .set(&provider_key(name), &serde_json::to_string(&credential)?)
    }

    /// Store a provider OAuth token.
    pub fn store_oauth(
        &self,
        name: &str,
        access: &str,
        refresh: Option<&str>,
        expires_at: Option<u64>,
    ) -> Result<(), VaultError> {
        let credential = Credential::OAuth {
            access_token: access.to_string(),
            refresh_token: refresh.map(str::to_string),
            expires_at,
        };
        self.backend
            .set(&provider_key(name), &serde_json::to_string(&credential)?)
    }

    /// Delete a provider credential. Returns `true` when a credential existed.
    pub fn remove_provider(&self, name: &str) -> Result<bool, VaultError> {
        let key = provider_key(name);
        if self.backend.get(&key)?.is_none() {
            return Ok(false);
        }
        self.backend.delete(&key)?;
        Ok(true)
    }

    /// List all provider credentials, for status display.
    pub fn list_providers(&self) -> Result<Vec<ProviderEntry>, VaultError> {
        let mut entries = Vec::new();
        for key in self.backend.list()? {
            let Some(name) = key.strip_prefix(PROVIDER_PREFIX) else {
                continue;
            };
            let Some(raw) = self.backend.get(&key)? else {
                continue;
            };
            let Ok(credential) = serde_json::from_str::<Credential>(&raw) else {
                continue;
            };
            let (credential_type, expires_at) = match credential {
                Credential::ApiKey { .. } => (CredentialType::ApiKey, None),
                Credential::OAuth { expires_at, .. } => (CredentialType::OAuth, expires_at),
            };
            entries.push(ProviderEntry {
                name: name.to_string(),
                credential_type,
                expires_at,
            });
        }
        Ok(entries)
    }

    // --- MCP OAuth Credentials ---

    /// Load the MCP OAuth credentials for a server.
    pub fn get_mcp_credentials(
        &self,
        server: &str,
    ) -> Result<Option<StoredCredentials>, VaultError> {
        let Some(raw) = self.backend.get(&mcp_cred_key(server))? else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_str(&raw)?))
    }

    /// Save the MCP OAuth credentials for a server.
    pub fn set_mcp_credentials(
        &self,
        server: &str,
        creds: &StoredCredentials,
    ) -> Result<(), VaultError> {
        self.backend
            .set(&mcp_cred_key(server), &serde_json::to_string(creds)?)
    }

    /// Clear the MCP OAuth credentials for a server.
    pub fn clear_mcp_credentials(&self, server: &str) -> Result<(), VaultError> {
        self.backend.delete(&mcp_cred_key(server))
    }

    // --- MCP OAuth State ---

    /// Load the PKCE/CSRF authorization state for a CSRF token.
    pub fn get_mcp_state(
        &self,
        csrf: &str,
    ) -> Result<Option<StoredAuthorizationState>, VaultError> {
        let Some(raw) = self.backend.get(&mcp_state_key(csrf))? else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_str(&raw)?))
    }

    /// Save the PKCE/CSRF authorization state for a CSRF token.
    pub fn set_mcp_state(
        &self,
        csrf: &str,
        state: &StoredAuthorizationState,
    ) -> Result<(), VaultError> {
        self.backend
            .set(&mcp_state_key(csrf), &serde_json::to_string(state)?)
    }

    /// Delete the PKCE/CSRF authorization state for a CSRF token.
    pub fn delete_mcp_state(&self, csrf: &str) -> Result<(), VaultError> {
        self.backend.delete(&mcp_state_key(csrf))
    }

    // --- MCP Bearer Tokens ---

    /// Resolve a bearer token. A `store:` value is resolved from the vault;
    /// any other value is returned unchanged.
    pub fn resolve_bearer(&self, token: &str) -> Result<String, VaultError> {
        let Some(name) = token.strip_prefix(STORE_PREFIX) else {
            return Ok(token.to_string());
        };
        let key = mcp_bearer_key(name);
        self.backend.get(&key)?.ok_or(VaultError::KeyNotFound(key))
    }

    // --- rmcp Adapters ---

    /// Produce an rmcp `CredentialStore` adapter for one MCP server.
    pub fn mcp_credential_store(self: &Arc<Self>, server: &str) -> VaultCredentialStore {
        VaultCredentialStore {
            vault: Arc::clone(self),
            server_name: server.to_string(),
        }
    }

    /// Produce an rmcp `StateStore` adapter.
    pub fn mcp_state_store(self: &Arc<Self>) -> VaultStateStore {
        VaultStateStore {
            vault: Arc::clone(self),
        }
    }
}

// endregion: --- Vault Impl

// region:    --- VaultBackendKind Impl

impl VaultBackend for VaultBackendKind {
    fn get(&self, key: &str) -> Result<Option<String>, VaultError> {
        match self {
            Self::Keychain(b) => b.get(key),
            Self::File(b) => b.get(key),
            Self::EnvOnly(b) => b.get(key),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), VaultError> {
        match self {
            Self::Keychain(b) => b.set(key, value),
            Self::File(b) => b.set(key, value),
            Self::EnvOnly(b) => b.set(key, value),
        }
    }

    fn delete(&self, key: &str) -> Result<(), VaultError> {
        match self {
            Self::Keychain(b) => b.delete(key),
            Self::File(b) => b.delete(key),
            Self::EnvOnly(b) => b.delete(key),
        }
    }

    fn list(&self) -> Result<Vec<String>, VaultError> {
        match self {
            Self::Keychain(b) => b.list(),
            Self::File(b) => b.list(),
            Self::EnvOnly(b) => b.list(),
        }
    }
}

// endregion: --- VaultBackendKind Impl

// region:    --- Support

/// Backend key for a provider credential.
fn provider_key(name: &str) -> String {
    format!("{PROVIDER_PREFIX}{name}")
}

/// Backend key for MCP OAuth credentials.
fn mcp_cred_key(server: &str) -> String {
    format!("{MCP_CRED_PREFIX}{server}")
}

/// Backend key for MCP OAuth PKCE/CSRF state.
fn mcp_state_key(csrf: &str) -> String {
    format!("{MCP_STATE_PREFIX}{csrf}")
}

/// Backend key for an MCP bearer token.
fn mcp_bearer_key(server: &str) -> String {
    format!("{MCP_BEARER_PREFIX}{server}")
}

/// Translate a vault failure into the rmcp auth error type.
fn to_auth_error(error: VaultError) -> AuthError {
    AuthError::InternalError(error.to_string())
}

// endregion: --- Support

// region:    --- rmcp Adapters

/// rmcp `CredentialStore` adapter for a single MCP server.
///
/// Holds a shared reference to the vault and the server name. Every rmcp call
/// delegates to the vault's typed MCP-credential methods.
pub struct VaultCredentialStore {
    pub vault: Arc<Vault>,
    pub server_name: String,
}

#[async_trait::async_trait]
impl CredentialStore for VaultCredentialStore {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        self.vault
            .get_mcp_credentials(&self.server_name)
            .map_err(to_auth_error)
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        self.vault
            .set_mcp_credentials(&self.server_name, &credentials)
            .map_err(to_auth_error)
    }

    async fn clear(&self) -> Result<(), AuthError> {
        self.vault
            .clear_mcp_credentials(&self.server_name)
            .map_err(to_auth_error)
    }
}

/// rmcp `StateStore` adapter.
///
/// Holds a shared reference to the vault. Every rmcp call delegates to the
/// vault's typed MCP-state methods.
pub struct VaultStateStore {
    pub vault: Arc<Vault>,
}

#[async_trait::async_trait]
impl StateStore for VaultStateStore {
    async fn save(
        &self,
        csrf_token: &str,
        state: StoredAuthorizationState,
    ) -> Result<(), AuthError> {
        self.vault
            .set_mcp_state(csrf_token, &state)
            .map_err(to_auth_error)
    }

    async fn load(&self, csrf_token: &str) -> Result<Option<StoredAuthorizationState>, AuthError> {
        self.vault.get_mcp_state(csrf_token).map_err(to_auth_error)
    }

    async fn delete(&self, csrf_token: &str) -> Result<(), AuthError> {
        self.vault
            .delete_mcp_state(csrf_token)
            .map_err(to_auth_error)
    }
}

// endregion: --- rmcp Adapters

// region:    --- Tests

#[cfg(test)]
#[path = "vault_test.rs"]
mod vault_test;

// endregion: --- Tests
