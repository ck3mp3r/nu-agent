//! OS keychain backend for the [`Vault`](super::vault::Vault).
//!
//! Every secret is one keychain entry under the service name [`SERVICE_NAME`],
//! with the vault key as the username.

// region:    --- Modules

use std::time::Duration;

use keyring::v1::Entry;

use super::vault::{BackendKind, VaultBackend, VaultError};

// endregion: --- Modules

// region:    --- Constants

/// Keychain service name. Every secret lives under this service. Debug builds
/// use `nu-agent-dev`, so they never touch production keychain entries.
pub const SERVICE_NAME: &str = crate::utils::paths::APP_DIR_NAME;

/// Registry index key. Holds a JSON array of every key stored in the keychain.
pub const INDEX_KEY: &str = "nu-agent:index";

/// Key read by [`KeychainBackend::probe`] to detect a credential store.
pub const PROBE_KEY: &str = "__vault_probe__";

/// Upper bound, in seconds, on one keychain probe.
///
/// The probe reaches the OS credential service through a synchronous call with
/// no cancellation point, so a parked service would block startup forever. Both
/// selection chains pass this value; tests pass a smaller deadline.
pub(crate) const KEYCHAIN_PROBE_TIMEOUT_SECS: u64 = 5;

// endregion: --- Constants

// region:    --- Types

/// OS keychain backend.
///
/// The `keyring` v1 API has no listing primitive, so a registry index entry
/// ([`INDEX_KEY`]) holds a JSON array of all keys. The index drives `list()`
/// only — a direct `get` never consults it.
#[derive(Debug)]
pub struct KeychainBackend;

// endregion: --- Types

// region:    --- KeychainBackend Impl

impl KeychainBackend {
    /// Probe whether an OS keychain credential store is available.
    ///
    /// Reads [`PROBE_KEY`]. Every outcome except "no default store" means a
    /// keychain is reachable — a missing probe entry included.
    pub fn probe() -> Result<Self, VaultError> {
        let entry = Entry::new(SERVICE_NAME, PROBE_KEY).map_err(map_error)?;
        match entry.get_password() {
            Ok(_) | Err(keyring::v1::Error::NoEntry) => Ok(Self),
            Err(e) => Err(map_error(e)),
        }
    }

    /// Run `probe` with a hard upper bound on the wait.
    ///
    /// The probe is a synchronous OS call with no cancellation point, so the
    /// bound is a thread plus a channel receive timeout: the probe runs on its
    /// own thread and the caller stops waiting once `deadline` passes.
    ///
    /// A probe that outlives its deadline leaves one parked thread behind. That
    /// is the accepted price of a bound that holds on every platform; reacting to
    /// the macOS interaction-required signal instead would cover macOS alone.
    /// Setting `AGENT_VAULT_BACKEND=env` or `AGENT_VAULT_BACKEND=file` skips the
    /// probe entirely, which is the escape the timeout message names.
    pub(crate) fn bounded_probe(
        deadline: Duration,
        probe: impl FnOnce() -> Result<Self, VaultError> + Send + 'static,
    ) -> Result<Self, VaultError> {
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = result_tx.send(probe());
        });
        match result_rx.recv_timeout(deadline) {
            Ok(result) => result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                Err(VaultError::BackendUnavailable(format!(
                    "keychain probe did not respond within {deadline:?} (the keychain service is \
                     parked or blocked; set AGENT_VAULT_BACKEND=env|file to proceed)"
                )))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(VaultError::Keychain(
                "the keychain probe thread ended without a result".to_string(),
            )),
        }
    }
}

// endregion: --- KeychainBackend Impl

// region:    --- VaultBackend Impl

impl VaultBackend for KeychainBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Keychain
    }

    fn get(&self, key: &str) -> Result<Option<String>, VaultError> {
        let entry = Entry::new(SERVICE_NAME, key).map_err(map_error)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::v1::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_error(e)),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), VaultError> {
        let entry = Entry::new(SERVICE_NAME, key).map_err(map_error)?;
        entry.set_password(value).map_err(map_error)?;
        // The index is only for status display, so a lost update is tolerable.
        index_add(key)
    }

    fn delete(&self, key: &str) -> Result<(), VaultError> {
        let entry = Entry::new(SERVICE_NAME, key).map_err(map_error)?;
        // Deleting an absent key is not an error.
        match entry.delete_credential() {
            Ok(()) | Err(keyring::v1::Error::NoEntry) => {}
            Err(e) => return Err(map_error(e)),
        }
        index_remove(key)
    }

    fn list(&self) -> Result<Vec<String>, VaultError> {
        read_index()
    }
}

// endregion: --- VaultBackend Impl

// region:    --- Support

/// Read the registry index. A missing index reads as empty.
fn read_index() -> Result<Vec<String>, VaultError> {
    let entry = Entry::new(SERVICE_NAME, INDEX_KEY).map_err(map_error)?;
    match entry.get_password() {
        Ok(raw) => Ok(serde_json::from_str(&raw)?),
        Err(keyring::v1::Error::NoEntry) => Ok(Vec::new()),
        Err(e) => Err(map_error(e)),
    }
}

/// Write the registry index.
fn write_index(keys: &[String]) -> Result<(), VaultError> {
    let raw = serde_json::to_string(keys)?;
    let entry = Entry::new(SERVICE_NAME, INDEX_KEY).map_err(map_error)?;
    entry.set_password(&raw).map_err(map_error)
}

/// Add a key to the registry index. Idempotent.
fn index_add(key: &str) -> Result<(), VaultError> {
    let current = read_index()?;
    let next = added(&current, key);
    if next == current {
        return Ok(());
    }
    write_index(&next)
}

/// Remove a key from the registry index. Idempotent.
fn index_remove(key: &str) -> Result<(), VaultError> {
    let current = read_index()?;
    let next = removed(&current, key);
    if next == current {
        return Ok(());
    }
    write_index(&next)
}

/// Append `key` unless it is already present. Pure — no I/O.
fn added(keys: &[String], key: &str) -> Vec<String> {
    let mut next = keys.to_vec();
    if !next.iter().any(|existing| existing == key) {
        next.push(key.to_string());
    }
    next
}

/// Drop every occurrence of `key`. Pure — no I/O.
fn removed(keys: &[String], key: &str) -> Vec<String> {
    keys.iter()
        .filter(|existing| existing.as_str() != key)
        .cloned()
        .collect()
}

/// Map a `keyring` error to a [`VaultError`].
///
/// A missing default credential store means no backend is available. Every
/// other failure is reported as a keychain error.
fn map_error(error: keyring::v1::Error) -> VaultError {
    match error {
        keyring::v1::Error::NoDefaultStore => VaultError::NoBackend,
        other => VaultError::Keychain(other.to_string()),
    }
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/config/keychain_backend.rs"]
mod keychain_backend_test;

// endregion: --- Tests
