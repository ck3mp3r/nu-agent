//! MCP credential and state tests.

use oauth2::{CsrfToken, PkceCodeVerifier};
use rmcp::transport::auth::{StoredAuthorizationState, StoredCredentials};

use super::VaultBackend;
use super::VaultError;
use super::vault_test_support::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_vault_mcp_credentials_round_trip() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault()?;
    let creds =
        StoredCredentials::new("client-1".to_string(), None, vec!["read".to_string()], None);

    // -- Exec
    vault.set_mcp_credentials("server-a", &creds)?;
    let loaded = vault.get_mcp_credentials("server-a")?;

    // -- Check
    let stored = loaded.ok_or("should have credentials")?;
    assert_eq!(stored.client_id, "client-1");
    assert_eq!(stored.granted_scopes, vec!["read".to_string()]);
    Ok(())
}

#[test]
fn test_vault_get_mcp_credentials_returns_none_when_absent() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault()?;

    // -- Exec
    let loaded = vault.get_mcp_credentials("server-a")?;

    // -- Check
    assert!(loaded.is_none());
    Ok(())
}

#[test]
fn test_vault_clear_mcp_credentials_removes_entry() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault()?;
    let creds = StoredCredentials::new("client-1".to_string(), None, vec![], None);
    vault.set_mcp_credentials("server-a", &creds)?;

    // -- Exec
    vault.clear_mcp_credentials("server-a")?;
    let loaded = vault.get_mcp_credentials("server-a")?;

    // -- Check
    assert!(loaded.is_none(), "cleared credentials are gone");
    Ok(())
}

#[test]
fn test_vault_mcp_state_round_trip() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault()?;
    let state = StoredAuthorizationState::new(
        &PkceCodeVerifier::new("pkce-verifier".to_string()),
        &CsrfToken::new("csrf-token".to_string()),
    );

    // -- Exec
    vault.set_mcp_state("csrf-token", &state)?;
    let loaded = vault.get_mcp_state("csrf-token")?;

    // -- Check
    let stored = loaded.ok_or("should have state")?;
    assert_eq!(stored.pkce_verifier, "pkce-verifier");
    assert_eq!(stored.csrf_token, "csrf-token");
    Ok(())
}

#[test]
fn test_vault_delete_mcp_state_removes_entry() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault()?;
    let state = StoredAuthorizationState::new(
        &PkceCodeVerifier::new("pkce-verifier".to_string()),
        &CsrfToken::new("csrf-token".to_string()),
    );
    vault.set_mcp_state("csrf-token", &state)?;

    // -- Exec
    vault.delete_mcp_state("csrf-token")?;
    let loaded = vault.get_mcp_state("csrf-token")?;

    // -- Check
    assert!(loaded.is_none(), "deleted state is gone");
    Ok(())
}

#[test]
fn test_vault_resolve_bearer_returns_literal_unchanged() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault()?;

    // -- Exec
    let resolved = vault.resolve_bearer("sk-raw-token")?;

    // -- Check
    assert_eq!(resolved, "sk-raw-token");
    Ok(())
}

#[test]
fn test_vault_resolve_bearer_resolves_store_reference() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault()?;
    vault.backend().set("mcp:bearer:server-a", "secret-token")?;

    // -- Exec
    let resolved = vault.resolve_bearer("store:server-a")?;

    // -- Check
    assert_eq!(resolved, "secret-token");
    Ok(())
}

#[test]
fn test_vault_resolve_bearer_missing_store_reference_is_key_not_found() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault()?;

    // -- Exec
    let resolved = vault.resolve_bearer("store:missing");

    // -- Check
    assert!(
        matches!(resolved, Err(VaultError::KeyNotFound(ref k)) if k == "mcp:bearer:missing"),
        "a missing store: reference reports KeyNotFound with the full key"
    );
    Ok(())
}

// endregion: --- Tests
