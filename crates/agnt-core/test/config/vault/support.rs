//! Shared fixtures for the vault tests.

use std::sync::Arc;

use tempfile::TempDir;

use crate::config::file_backend::FileBackend;

use super::{Vault, VaultBackendKind};

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// A vault backed by a `FileBackend` inside a temp dir.
///
/// The `TempDir` is returned so the caller keeps it alive for the test's
/// duration; dropping it removes the secrets file.
pub(super) fn temp_vault() -> Result<(TempDir, Vault)> {
    let dir = TempDir::new()?;
    let backend = FileBackend::new(dir.path().join("secrets.json"));
    let vault = Vault::new(VaultBackendKind::File(backend));
    Ok((dir, vault))
}

/// As [`temp_vault`], but shared behind `Arc` for the rmcp adapters.
pub(super) fn temp_vault_arc() -> Result<(TempDir, Arc<Vault>)> {
    let dir = TempDir::new()?;
    let backend = FileBackend::new(dir.path().join("secrets.json"));
    let vault = Vault::new(VaultBackendKind::File(backend));
    Ok((dir, Arc::new(vault)))
}

// endregion: --- Test Support
