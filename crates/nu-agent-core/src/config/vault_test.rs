use std::sync::Arc;

use oauth2::{CsrfToken, PkceCodeVerifier};
use rmcp::transport::auth::{
    CredentialStore, StateStore, StoredAuthorizationState, StoredCredentials,
};
use tempfile::TempDir;

use crate::config::file_backend::FileBackend;
use crate::config::keychain_backend::KeychainBackend;

use super::{
    Credential, CredentialType, EnvOnlyBackend, ProviderEntry, Vault, VaultBackend,
    VaultBackendKind, VaultError,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_vault_env_only_constructs_env_only_backend() -> Result<()> {
    // -- Setup & Fixtures
    // (no fixtures required — the constructor takes no arguments)

    // -- Exec
    let vault = Vault::env_only();

    // -- Check
    assert!(
        matches!(vault.backend(), VaultBackendKind::EnvOnly(_)),
        "env_only() should select the EnvOnly backend"
    );
    Ok(())
}

#[test]
fn test_vault_new_uses_given_backend() -> Result<()> {
    // -- Setup & Fixtures
    let backend = VaultBackendKind::EnvOnly(EnvOnlyBackend);

    // -- Exec
    let vault = Vault::new(backend);

    // -- Check
    assert!(
        matches!(vault.backend(), VaultBackendKind::EnvOnly(_)),
        "new() should keep the backend it was given"
    );
    Ok(())
}

#[test]
fn test_vault_backend_kind_has_three_variants() -> Result<()> {
    // -- Setup & Fixtures
    let variants = [
        VaultBackendKind::Keychain(KeychainBackend),
        VaultBackendKind::File(FileBackend::new(std::path::PathBuf::from("/nonexistent"))),
        VaultBackendKind::EnvOnly(EnvOnlyBackend),
    ];

    // -- Exec
    let count = variants.len();

    // -- Check
    assert_eq!(count, 3, "VaultBackendKind should have exactly 3 variants");
    Ok(())
}

#[test]
fn test_vault_error_keychain_display_contains_message() -> Result<()> {
    // -- Setup & Fixtures
    let error = VaultError::Keychain("test".to_string());

    // -- Exec
    let rendered = error.to_string();

    // -- Check
    assert!(
        rendered.contains("Keychain error: test"),
        "Display should contain the keychain message, got: {rendered}"
    );
    Ok(())
}

#[test]
fn test_vault_error_key_not_found_display_contains_key_name() -> Result<()> {
    // -- Setup & Fixtures
    let error = VaultError::KeyNotFound("mcp:bearer:missing".to_string());

    // -- Exec
    let rendered = error.to_string();

    // -- Check
    assert!(
        rendered.contains("mcp:bearer:missing"),
        "Display should contain the missing key name, got: {rendered}"
    );
    Ok(())
}

#[test]
fn test_vault_error_remaining_variants_display() -> Result<()> {
    // -- Setup & Fixtures
    let errors = [
        (
            VaultError::Io(std::io::Error::other("boom")),
            "I/O error: boom",
        ),
        (VaultError::NoBackend, "No vault backend available"),
        (
            VaultError::NoDataDir,
            "No data directory found — set XDG_DATA_HOME",
        ),
    ];

    // -- Exec & Check
    for (error, expected) in errors {
        let rendered = error.to_string();
        assert_eq!(rendered, expected, "unexpected Display output");
    }
    Ok(())
}

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
fn test_credential_serializes_with_type_tag() -> Result<()> {
    // -- Setup & Fixtures
    let api_key = Credential::ApiKey {
        key: "sk-test".to_string(),
    };
    let oauth = Credential::OAuth {
        access_token: "access".to_string(),
        refresh_token: Some("refresh".to_string()),
        expires_at: Some(42),
    };

    // -- Exec
    let api_key_json = serde_json::to_string(&api_key)?;
    let oauth_json = serde_json::to_string(&oauth)?;

    // -- Check
    assert!(api_key_json.contains("\"type\":\"ApiKey\""));
    assert!(oauth_json.contains("\"type\":\"OAuth\""));
    Ok(())
}

#[test]
fn test_provider_entry_holds_credential_type() -> Result<()> {
    // -- Setup & Fixtures
    let entry = ProviderEntry {
        name: "openai".to_string(),
        credential_type: CredentialType::ApiKey,
        expires_at: None,
    };

    // -- Exec
    let name = entry.name.clone();
    let credential_type = entry.credential_type;

    // -- Check
    assert_eq!(name, "openai");
    assert_eq!(credential_type, CredentialType::ApiKey);
    Ok(())
}

#[test]
fn test_vault_store_api_key_then_resolve() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault()?;

    // -- Exec
    vault.store_api_key("openai", "sk-test")?;
    let resolved = vault.resolve("store:openai");

    // -- Check
    assert_eq!(resolved.as_deref(), Some("sk-test"));
    Ok(())
}

#[test]
fn test_vault_resolve_returns_none_for_missing_key() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault()?;

    // -- Exec
    let missing = vault.resolve("store:unknown");

    // -- Check
    assert_eq!(missing, None);
    Ok(())
}

#[test]
fn test_vault_resolve_returns_none_for_non_store_reference() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault()?;

    // -- Exec
    let literal = vault.resolve("sk-raw-literal");

    // -- Check
    assert_eq!(literal, None, "a non-store reference is not resolved");
    Ok(())
}

#[test]
fn test_vault_store_oauth_then_resolve_returns_access_token() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault()?;

    // -- Exec
    vault.store_oauth(
        "github-copilot",
        "gho-access",
        Some("ghr-refresh"),
        Some(99),
    )?;
    let resolved = vault.resolve("store:github-copilot");

    // -- Check
    assert_eq!(resolved.as_deref(), Some("gho-access"));
    Ok(())
}

#[test]
fn test_vault_list_providers_reports_api_key_entry() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault()?;
    vault.store_api_key("openai", "sk-test")?;

    // -- Exec
    let entries = vault.list_providers()?;

    // -- Check
    assert_eq!(entries.len(), 1, "one provider is listed");
    let entry = entries.first().ok_or("should have one entry")?;
    assert_eq!(entry.name, "openai");
    assert_eq!(entry.credential_type, CredentialType::ApiKey);
    assert_eq!(entry.expires_at, None);
    Ok(())
}

#[test]
fn test_vault_list_providers_reports_oauth_entry_with_expiry() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault()?;
    vault.store_oauth("copilot", "access", None, Some(1234))?;

    // -- Exec
    let entries = vault.list_providers()?;

    // -- Check
    let entry = entries.first().ok_or("should have one entry")?;
    assert_eq!(entry.credential_type, CredentialType::OAuth);
    assert_eq!(entry.expires_at, Some(1234));
    Ok(())
}

#[test]
fn test_vault_list_providers_ignores_non_provider_keys() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault()?;
    vault.store_api_key("openai", "sk-test")?;
    vault.backend().set("mcp:bearer:server", "token")?;

    // -- Exec
    let entries = vault.list_providers()?;

    // -- Check
    assert_eq!(entries.len(), 1, "only provider: keys are listed");
    Ok(())
}

#[test]
fn test_vault_remove_provider_returns_true_and_deletes() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault()?;
    vault.store_api_key("openai", "sk-test")?;

    // -- Exec
    let removed = vault.remove_provider("openai")?;

    // -- Check
    assert!(removed, "remove_provider reports the credential existed");
    assert_eq!(vault.resolve("store:openai"), None, "the key is gone");
    Ok(())
}

#[test]
fn test_vault_remove_provider_returns_false_when_absent() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault()?;

    // -- Exec
    let removed = vault.remove_provider("nonexistent")?;

    // -- Check
    assert!(!removed, "remove_provider reports nothing to remove");
    Ok(())
}

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

// region:    --- Test Support

/// A vault backed by a `FileBackend` inside a temp dir.
///
/// The `TempDir` is returned so the caller keeps it alive for the test's
/// duration; dropping it removes the secrets file.
fn temp_vault() -> Result<(TempDir, Vault)> {
    let dir = TempDir::new()?;
    let backend = FileBackend::new(dir.path().join("secrets.json"));
    let vault = Vault::new(VaultBackendKind::File(backend));
    Ok((dir, vault))
}

/// As [`temp_vault`], but shared behind `Arc` for the rmcp adapters.
fn temp_vault_arc() -> Result<(TempDir, Arc<Vault>)> {
    let dir = TempDir::new()?;
    let backend = FileBackend::new(dir.path().join("secrets.json"));
    let vault = Vault::new(VaultBackendKind::File(backend));
    Ok((dir, Arc::new(vault)))
}

// endregion: --- Test Support

// endregion: --- Tests
