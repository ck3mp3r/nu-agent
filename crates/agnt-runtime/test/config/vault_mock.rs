use std::collections::HashMap;
use std::sync::Mutex;

use super::{BackendKind, Vault, VaultBackend, VaultError};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// In-memory backend that proves the injection seam and injects errors.
///
/// `failure` is `None` for a working store and `Some(msg)` for a backend whose
/// every operation fails with `VaultError::Keychain(msg)`.
struct MockBackend {
    entries: Mutex<HashMap<String, String>>,
    failure: Option<String>,
}

impl MockBackend {
    fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            failure: None,
        }
    }

    fn failing(msg: &str) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            failure: Some(msg.to_string()),
        }
    }

    fn injected(&self) -> Option<VaultError> {
        self.failure
            .as_ref()
            .map(|msg| VaultError::Keychain(msg.clone()))
    }
}

impl VaultBackend for MockBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::File
    }

    fn get(&self, key: &str) -> core::result::Result<Option<String>, VaultError> {
        if let Some(error) = self.injected() {
            return Err(error);
        }
        Ok(self.entries.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> core::result::Result<(), VaultError> {
        if let Some(error) = self.injected() {
            return Err(error);
        }
        self.entries
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> core::result::Result<(), VaultError> {
        if let Some(error) = self.injected() {
            return Err(error);
        }
        self.entries.lock().unwrap().remove(key);
        Ok(())
    }

    fn list(&self) -> core::result::Result<Vec<String>, VaultError> {
        if let Some(error) = self.injected() {
            return Err(error);
        }
        Ok(self.entries.lock().unwrap().keys().cloned().collect())
    }
}

// endregion: --- Test Support

// region:    --- Tests

#[test]
fn test_vault_mock_store_api_key_then_resolve_round_trip() -> Result<()> {
    // -- Setup & Fixtures
    let vault = Vault::new(MockBackend::new());

    // -- Exec
    vault.store_api_key("openai", "sk-mock")?;
    let resolved = vault.resolve("store:openai");

    // -- Check
    assert_eq!(
        resolved.as_deref(),
        Some("sk-mock"),
        "a non-enum backend must flow through the provider-credential family"
    );
    Ok(())
}

#[test]
fn test_vault_mock_store_api_key_propagates_backend_error() -> Result<()> {
    // -- Setup & Fixtures
    let vault = Vault::new(MockBackend::failing("injected"));

    // -- Exec
    let stored = vault.store_api_key("openai", "sk-mock");

    // -- Check
    assert!(
        matches!(&stored, Err(VaultError::Keychain(msg)) if msg == "injected"),
        "a failing backend must surface its error unchanged, got: {stored:?}"
    );
    Ok(())
}

#[test]
fn test_vault_mock_get_mcp_credentials_propagates_backend_error() -> Result<()> {
    // -- Setup & Fixtures
    let vault = Vault::new(MockBackend::failing("injected"));

    // -- Exec
    let loaded = vault.get_mcp_credentials("server-a");

    // -- Check
    assert!(
        matches!(&loaded, Err(VaultError::Keychain(msg)) if msg == "injected"),
        "a failing backend must surface its error unchanged, got: {loaded:?}"
    );
    Ok(())
}

#[test]
fn test_vault_mock_list_providers_propagates_backend_error() -> Result<()> {
    // -- Setup & Fixtures
    let vault = Vault::new(MockBackend::failing("injected"));

    // -- Exec
    let listed = vault.list_providers();

    // -- Check
    assert!(
        matches!(&listed, Err(VaultError::Keychain(msg)) if msg == "injected"),
        "a failing backend must surface its error unchanged, got: {listed:?}"
    );
    Ok(())
}

#[test]
fn test_vault_mock_delete_mcp_state_propagates_backend_error() -> Result<()> {
    // -- Setup & Fixtures
    let vault = Vault::new(MockBackend::failing("injected"));

    // -- Exec
    let deleted = vault.delete_mcp_state("csrf-token");

    // -- Check
    assert!(
        matches!(&deleted, Err(VaultError::Keychain(msg)) if msg == "injected"),
        "a failing backend must surface its error unchanged, got: {deleted:?}"
    );
    Ok(())
}

#[test]
fn test_vault_mock_resolve_bearer_literal_skips_failing_backend() -> Result<()> {
    // -- Setup & Fixtures
    let vault = Vault::new(MockBackend::failing("injected"));

    // -- Exec
    let resolved = vault.resolve_bearer("sk-raw-token")?;

    // -- Check
    assert_eq!(
        resolved, "sk-raw-token",
        "a literal bearer must return unchanged without touching the backend"
    );
    Ok(())
}

// endregion: --- Tests
