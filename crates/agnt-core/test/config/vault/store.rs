//! Credential store and provider resolution tests.

use super::vault_test_support::*;
use super::{Credential, CredentialType, ProviderEntry, VaultBackend};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

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

// endregion: --- Tests
