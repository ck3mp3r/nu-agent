//! Env-only backend operation tests.

use rmcp::transport::auth::StoredCredentials;

use super::{EnvOnlyBackend, Vault, VaultBackend, VaultBackendKind, VaultError};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_vault_backend_trait_dispatch_reaches_env_only_backend() -> Result<()> {
    // -- Setup & Fixtures
    // An env-only backend never panics, so a successful no-op through the
    // enum match arms proves the dispatch reaches the concrete backend.
    let backend = VaultBackendKind::EnvOnly(EnvOnlyBackend);

    // -- Exec
    let fetched = backend.get("key")?;
    let listed = backend.list()?;

    // -- Check
    assert_eq!(fetched, None, "env-only get returns None");
    assert!(listed.is_empty(), "env-only list returns empty");
    Ok(())
}

#[test]
fn test_vault_env_only_set_returns_no_backend() -> Result<()> {
    // -- Setup & Fixtures
    let vault = Vault::env_only();

    // -- Exec
    let stored = vault.store_api_key("openai", "sk-test");

    // -- Check
    assert!(
        matches!(&stored, Err(VaultError::NoBackend)),
        "an env-only vault must fail a write with NoBackend, got: {stored:?}"
    );
    Ok(())
}

#[test]
fn test_vault_env_only_set_mcp_credentials_returns_no_backend() -> Result<()> {
    // -- Setup & Fixtures
    let vault = Vault::env_only();
    let creds = StoredCredentials::new("client-1".to_string(), None, vec![], None);

    // -- Exec
    let stored = vault.set_mcp_credentials("server-a", &creds);

    // -- Check
    assert!(
        matches!(&stored, Err(VaultError::NoBackend)),
        "an env-only vault must fail an MCP credential write with NoBackend, got: {stored:?}"
    );
    Ok(())
}

#[test]
fn test_vault_env_only_get_returns_none() -> Result<()> {
    // -- Setup & Fixtures
    let vault = Vault::env_only();

    // -- Exec
    let fetched = vault.backend().get("provider:openai")?;

    // -- Check
    assert_eq!(fetched, None, "env-only get returns None");
    Ok(())
}

#[test]
fn test_vault_env_only_list_returns_empty() -> Result<()> {
    // -- Setup & Fixtures
    let vault = Vault::env_only();

    // -- Exec
    let listed = vault.backend().list()?;

    // -- Check
    assert!(listed.is_empty(), "env-only list returns empty");
    Ok(())
}

#[test]
fn test_vault_env_only_delete_is_ok() -> Result<()> {
    // -- Setup & Fixtures
    let vault = Vault::env_only();

    // -- Exec
    let deleted = vault.backend().delete("provider:openai");

    // -- Check
    assert!(deleted.is_ok(), "env-only delete stays a no-op Ok");
    Ok(())
}

// endregion: --- Tests
