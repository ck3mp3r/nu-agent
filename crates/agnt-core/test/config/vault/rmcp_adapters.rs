//! rmcp CredentialStore and StateStore adapter tests.

use oauth2::{CsrfToken, PkceCodeVerifier};
use rmcp::transport::auth::{
    CredentialStore, StateStore, StoredAuthorizationState, StoredCredentials,
};

use super::vault_test_support::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[tokio::test]
async fn test_vault_credential_store_save_then_load_round_trip() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault_arc()?;
    let store = vault.mcp_credential_store("server-a");
    let creds = StoredCredentials::new(
        "client-1".to_string(),
        None,
        vec!["read".to_string()],
        Some(42),
    );

    // -- Exec
    store.save(creds).await?;
    let loaded = store.load().await?;

    // -- Check
    let stored = loaded.ok_or("should have credentials")?;
    assert_eq!(stored.client_id, "client-1");
    assert_eq!(stored.granted_scopes, vec!["read".to_string()]);
    assert_eq!(stored.token_received_at, Some(42));
    Ok(())
}

#[tokio::test]
async fn test_vault_credential_store_load_none_when_absent() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault_arc()?;
    let store = vault.mcp_credential_store("server-a");

    // -- Exec
    let loaded = store.load().await?;

    // -- Check
    assert!(loaded.is_none());
    Ok(())
}

#[tokio::test]
async fn test_vault_credential_store_clear_removes_entry() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault_arc()?;
    let store = vault.mcp_credential_store("server-a");
    let creds = StoredCredentials::new("client-1".to_string(), None, vec![], None);
    store.save(creds).await?;

    // -- Exec
    store.clear().await?;
    let loaded = store.load().await?;

    // -- Check
    assert!(loaded.is_none(), "cleared credentials are gone");
    Ok(())
}

#[tokio::test]
async fn test_vault_credential_store_is_scoped_to_one_server() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault_arc()?;
    let store_a = vault.mcp_credential_store("server-a");
    let store_b = vault.mcp_credential_store("server-b");
    let creds = StoredCredentials::new("client-a".to_string(), None, vec![], None);

    // -- Exec
    store_a.save(creds).await?;
    let loaded_b = store_b.load().await?;

    // -- Check
    assert!(
        loaded_b.is_none(),
        "a store only sees credentials for its own server"
    );
    Ok(())
}

#[tokio::test]
async fn test_vault_state_store_save_then_load_round_trip() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault_arc()?;
    let store = vault.mcp_state_store();
    let state = StoredAuthorizationState::new(
        &PkceCodeVerifier::new("pkce-verifier".to_string()),
        &CsrfToken::new("csrf-token".to_string()),
    );

    // -- Exec
    store.save("csrf-token", state).await?;
    let loaded = store.load("csrf-token").await?;

    // -- Check
    let stored = loaded.ok_or("should have state")?;
    assert_eq!(stored.pkce_verifier, "pkce-verifier");
    assert_eq!(stored.csrf_token, "csrf-token");
    Ok(())
}

#[tokio::test]
async fn test_vault_state_store_delete_removes_entry() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault_arc()?;
    let store = vault.mcp_state_store();
    let state = StoredAuthorizationState::new(
        &PkceCodeVerifier::new("pkce-verifier".to_string()),
        &CsrfToken::new("csrf-token".to_string()),
    );
    store.save("csrf-token", state).await?;

    // -- Exec
    store.delete("csrf-token").await?;
    let loaded = store.load("csrf-token").await?;

    // -- Check
    assert!(loaded.is_none(), "deleted state is gone");
    Ok(())
}

// endregion: --- Tests
