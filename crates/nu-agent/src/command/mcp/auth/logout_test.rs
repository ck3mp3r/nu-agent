//! Tests for the MCP auth logout command.

use std::sync::Arc;

use rmcp::transport::auth::StoredCredentials;
use tempfile::TempDir;

use nu_agent_core::config::file_backend::FileBackend;
use nu_agent_core::config::vault::{Vault, VaultBackendKind};

use super::logout::perform_logout;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A `FileBackend`-backed vault inside a temp dir, with one stored credential
/// for `server_name`.
fn vault_with_entry(server_name: &str) -> Result<(TempDir, Arc<Vault>)> {
    let dir = TempDir::new()?;
    let backend = FileBackend::new(dir.path().join("secrets.json"));
    let vault = Vault::new(VaultBackendKind::File(backend));
    let creds = StoredCredentials::new("client-1".to_string(), None, vec![], None);
    vault.set_mcp_credentials(server_name, &creds)?;
    Ok((dir, Arc::new(vault)))
}

#[test]
fn logout_clears_entry_from_vault() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = vault_with_entry("my-server")?;
    assert!(vault.get_mcp_credentials("my-server")?.is_some());

    // -- Exec
    let msg = perform_logout(&vault, "my-server");

    // -- Check
    assert!(vault.get_mcp_credentials("my-server")?.is_none());
    assert_eq!(msg, "Cleared credentials for 'my-server'");
    Ok(())
}

#[test]
fn logout_on_nonexistent_server_returns_no_credentials_message() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let backend = FileBackend::new(dir.path().join("secrets.json"));
    let vault = Vault::new(VaultBackendKind::File(backend));

    // -- Exec
    let msg = perform_logout(&vault, "unknown-server");

    // -- Check
    assert_eq!(msg, "No stored credentials for 'unknown-server'");
    Ok(())
}

#[test]
fn logout_persists_to_backend() -> Result<()> {
    // -- Setup & Fixtures
    let (dir, vault) = vault_with_entry("my-server")?;

    // -- Exec
    let _msg = perform_logout(&vault, "my-server");
    // A fresh vault over the same file must not see the cleared entry.
    let reopened = Vault::new(VaultBackendKind::File(FileBackend::new(
        dir.path().join("secrets.json"),
    )));

    // -- Check
    assert!(reopened.get_mcp_credentials("my-server")?.is_none());
    Ok(())
}

#[test]
fn logout_clears_only_target_server() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = vault_with_entry("server-a")?;
    let creds = StoredCredentials::new("client-1".to_string(), None, vec![], None);
    vault.set_mcp_credentials("server-b", &creds)?;
    vault.set_mcp_credentials("server-c", &creds)?;

    // -- Exec
    let msg = perform_logout(&vault, "server-b");

    // -- Check
    assert!(vault.get_mcp_credentials("server-a")?.is_some());
    assert!(vault.get_mcp_credentials("server-b")?.is_none());
    assert!(vault.get_mcp_credentials("server-c")?.is_some());
    assert_eq!(msg, "Cleared credentials for 'server-b'");
    Ok(())
}
