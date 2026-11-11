//! Vault — the single interface for secret storage and retrieval.
//!
//! Every consumer (provider auth commands, MCP auth commands, the config
//! resolver, and the MCP runtime) interacts with secrets through [`Vault`].
//! The storage backend is selected once at startup by [`Vault::auto_detect`]
//! and dispatched statically through the [`VaultBackend`] generic: `Vault<B>`
//! is monomorphized per backend, and tests inject a mock by naming it.

// region:    --- Modules

use std::path::Path;
use std::sync::Arc;

use crate::utils::env_map::{EnvMap, process_env};

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

/// Environment variable that overrides the vault backend selection.
const BACKEND_ENV: &str = "AGENT_VAULT_BACKEND";

/// Environment variable that nextest sets in every test process.
const NEXTEST_ENV: &str = "NEXTEST";

// endregion: --- Constants

// region:    --- Types

// -- Vault

/// The single point of interaction with secret storage and retrieval.
///
/// Owns a [`VaultBackend`] backend and exposes typed methods for each secret
/// category. Consumers never touch a backend directly.
///
/// The backend is a compile-time parameter: production runs the default
/// `Vault<VaultBackendKind>`; tests inject a mock by naming it.
#[derive(Debug)]
pub struct Vault<B: VaultBackend = VaultBackendKind> {
    backend: B,
}

/// Two vaults are equal when they use the same backend variant.
///
/// The comparison never inspects stored values — only the backend kind — so it
/// is safe to derive `PartialEq` for configuration comparison.
impl<B: VaultBackend> PartialEq for Vault<B> {
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
    /// Identifies the backend without exposing its internals.
    fn kind(&self) -> BackendKind;

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
    #[error(
        "keychain backend unavailable: {0} — unlock the keychain, or set AGENT_VAULT_BACKEND=file to use the plaintext file backend"
    )]
    BackendUnavailable(String),
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
/// Read paths are empty: `get` always returns `None`, `list` returns empty,
/// and `delete` is a no-op. `set` fails with [`VaultError::NoBackend`] — the
/// backend cannot store anything, so a write must not report success. It is
/// the unconditional final fallback of [`Vault::auto_detect`], for pure
/// Docker/CI environments where env vars are the only secret source.
#[derive(Debug)]
pub struct EnvOnlyBackend;

impl VaultBackend for EnvOnlyBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::EnvOnly
    }

    fn get(&self, _key: &str) -> Result<Option<String>, VaultError> {
        Ok(None)
    }

    fn set(&self, _key: &str, _value: &str) -> Result<(), VaultError> {
        Err(VaultError::NoBackend)
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

/// Runtime-selected vault: the production instantiation.
impl Vault<VaultBackendKind> {
    // --- Construction ---

    /// Select a backend from the process environment.
    ///
    /// `AGENT_VAULT_BACKEND` overrides the default chain: `env` selects the
    /// env-only backend without probing, `file` probes the file backend only,
    /// and `keychain` or `auto` probe keychain, then file, then env-only.
    ///
    /// `secrets_path` is the resolved secrets file
    /// ([`Paths::secrets_path`](crate::config::Paths::secrets_path)); the file
    /// backend writes exactly there.
    ///
    /// # Errors
    /// - [`VaultError::BackendUnavailable`] when the keychain probe fails with
    ///   anything other than [`VaultError::NoBackend`]
    pub fn auto_detect(secrets_path: &Path) -> Result<Self, VaultError> {
        Self::auto_detect_with(&process_env(), secrets_path)
    }

    /// Select a backend from an explicit environment map.
    ///
    /// The map-taking variant keeps the selection testable without mutating
    /// the process environment.
    fn auto_detect_with(env: &EnvMap, secrets_path: &Path) -> Result<Self, VaultError> {
        Ok(Self::new(resolve_backend(
            backend_choice(env),
            secrets_path,
        )?))
    }

    /// Construct an env-only vault. All lookups return `None`.
    pub fn env_only() -> Self {
        Self::new(VaultBackendKind::EnvOnly(EnvOnlyBackend))
    }
}

/// Backend-agnostic vault: every instantiation shares these methods.
impl<B: VaultBackend> Vault<B> {
    /// Construct a vault with a specific backend.
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    // --- Accessors ---

    /// Borrow the selected backend.
    pub fn backend(&self) -> &B {
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
    pub fn mcp_credential_store(self: &Arc<Self>, server: &str) -> VaultCredentialStore<B> {
        VaultCredentialStore {
            vault: Arc::clone(self),
            server_name: server.to_string(),
        }
    }

    /// Produce an rmcp `StateStore` adapter.
    pub fn mcp_state_store(self: &Arc<Self>) -> VaultStateStore<B> {
        VaultStateStore {
            vault: Arc::clone(self),
        }
    }
}

// endregion: --- Vault Impl

// region:    --- VaultBackendKind Impl

impl VaultBackend for VaultBackendKind {
    fn kind(&self) -> BackendKind {
        match self {
            Self::Keychain(_) => BackendKind::Keychain,
            Self::File(_) => BackendKind::File,
            Self::EnvOnly(_) => BackendKind::EnvOnly,
        }
    }

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

/// Which backend the environment requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendChoice {
    /// Probe keychain, then file, then env-only.
    Auto,
    /// Probe the file backend, then fall back to env-only.
    File,
    /// Use the env-only backend without probing.
    EnvOnly,
}

/// Map `AGENT_VAULT_BACKEND` to a backend choice. Pure — no probing.
///
/// `keychain` and `auto` share the auto chain: keychain, then file, then
/// env-only. An unknown value logs a warning and keeps the auto chain.
///
/// Under nextest ([`NEXTEST_ENV`]) an unset `AGENT_VAULT_BACKEND` selects the
/// env-only backend, so no test probes the OS keychain. An explicit value
/// always wins.
fn backend_choice(env: &EnvMap) -> BackendChoice {
    match env.get(BACKEND_ENV).map(String::as_str) {
        Some("env") => BackendChoice::EnvOnly,
        Some("file") => BackendChoice::File,
        Some("keychain") | Some("auto") => BackendChoice::Auto,
        Some(other) => {
            log::warn!("Unknown {BACKEND_ENV} value '{other}' — using auto detection");
            BackendChoice::Auto
        }
        None if env.contains_key(NEXTEST_ENV) => BackendChoice::EnvOnly,
        None => BackendChoice::Auto,
    }
}

/// Resolve a backend choice to a concrete backend, probing as needed.
fn resolve_backend(
    choice: BackendChoice,
    secrets_path: &Path,
) -> Result<VaultBackendKind, VaultError> {
    match choice {
        BackendChoice::Auto => auto_chain(secrets_path),
        choice => resolve_backend_with(choice, KeychainBackend::probe, || {
            FileBackend::probe(secrets_path)
        }),
    }
}

/// Resolve a backend choice using injectable probes.
fn resolve_backend_with(
    choice: BackendChoice,
    keychain_probe: impl FnOnce() -> Result<KeychainBackend, VaultError>,
    file_probe: impl FnOnce() -> Result<FileBackend, VaultError>,
) -> Result<VaultBackendKind, VaultError> {
    match choice {
        BackendChoice::EnvOnly => Ok(VaultBackendKind::EnvOnly(EnvOnlyBackend)),
        BackendChoice::File => match file_probe() {
            Ok(backend) => Ok(VaultBackendKind::File(backend)),
            Err(e) => {
                log::warn!("File vault backend unavailable ({e}) — falling back to env-only");
                Ok(VaultBackendKind::EnvOnly(EnvOnlyBackend))
            }
        },
        BackendChoice::Auto => auto_chain_with(keychain_probe, file_probe),
    }
}

/// Probe keychain, then file, then env-only. The first available backend wins.
///
/// A keychain probe failure other than [`VaultError::NoBackend`] aborts the
/// chain: silently downgrading to the file backend would store plaintext
/// secrets while the user believes the keychain holds them.
fn auto_chain(secrets_path: &Path) -> Result<VaultBackendKind, VaultError> {
    auto_chain_with(KeychainBackend::probe, || FileBackend::probe(secrets_path))
}

/// Probe keychain, then file, then env-only using injectable probes.
fn auto_chain_with(
    keychain_probe: impl FnOnce() -> Result<KeychainBackend, VaultError>,
    file_probe: impl FnOnce() -> Result<FileBackend, VaultError>,
) -> Result<VaultBackendKind, VaultError> {
    match keychain_probe() {
        Ok(keychain) => return Ok(VaultBackendKind::Keychain(keychain)),
        // No credential store at all — the designed container fallback.
        Err(VaultError::NoBackend) => {}
        Err(e) => return Err(VaultError::BackendUnavailable(e.to_string())),
    }
    if let Ok(file) = file_probe() {
        return Ok(VaultBackendKind::File(file));
    }
    Ok(VaultBackendKind::EnvOnly(EnvOnlyBackend))
}

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
pub struct VaultCredentialStore<B: VaultBackend = VaultBackendKind> {
    pub vault: Arc<Vault<B>>,
    pub server_name: String,
}

#[async_trait::async_trait]
impl<B: VaultBackend> CredentialStore for VaultCredentialStore<B> {
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
pub struct VaultStateStore<B: VaultBackend = VaultBackendKind> {
    pub vault: Arc<Vault<B>>,
}

#[async_trait::async_trait]
impl<B: VaultBackend> StateStore for VaultStateStore<B> {
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
#[path = "../../test/config/vault/support.rs"]
mod vault_test_support;

#[cfg(test)]
#[path = "../../test/config/vault/selection.rs"]
mod vault_selection_test;

#[cfg(test)]
#[path = "../../test/config/vault/errors.rs"]
mod vault_errors_test;

#[cfg(test)]
#[path = "../../test/config/vault/env_only.rs"]
mod vault_env_only_test;

#[cfg(test)]
#[path = "../../test/config/vault/store.rs"]
mod vault_store_test;

#[cfg(test)]
#[path = "../../test/config/vault/mcp.rs"]
mod vault_mcp_test;

#[cfg(test)]
#[path = "../../test/config/vault/rmcp_adapters.rs"]
mod vault_rmcp_test;

#[cfg(test)]
#[path = "../../test/config/vault_mock.rs"]
mod vault_mock_test;

#[cfg(test)]
#[path = "../../test/config/vault_probe.rs"]
mod vault_probe_test;

// endregion: --- Tests
