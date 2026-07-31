use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use oauth2::{CsrfToken, PkceCodeVerifier};
use rmcp::transport::auth::{
    CredentialStore, StateStore, StoredAuthorizationState, StoredCredentials,
};
use tempfile::TempDir;
use tokio::sync::Mutex;

use super::{CredentialsError, FileCredentialStore, FileStateStore, McpCredentialsStore};

fn make_store() -> (McpCredentialsStore, TempDir) {
    let dir = TempDir::new().expect("temp dir");
    let mut store = McpCredentialsStore::default();
    let entry = store.entries.entry("server-a".to_string()).or_default();
    entry.server_url = Some("https://example.com/mcp".to_string());
    entry.stored_credentials = Some(StoredCredentials::new(
        "client-1".to_string(),
        None,
        vec!["read".to_string()],
        Some(1000),
    ));
    (store, dir)
}

#[test]
fn load_non_existent_file_returns_empty_store() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("nonexistent.json");
    let store = McpCredentialsStore::load_from(&path).expect("load non-existent");
    assert!(store.entries.is_empty());
}

#[test]
fn save_and_reload_preserves_entries() {
    let (store, dir) = make_store();
    let path = dir.path().join("mcp-auth.json");

    store.save_to(&path).expect("save");

    let loaded = McpCredentialsStore::load_from(&path).expect("reload");
    assert_eq!(loaded.entries.len(), 1);

    let entry = loaded.entries.get("server-a").expect("server-a present");
    let creds = entry
        .stored_credentials
        .as_ref()
        .expect("stored_credentials present");
    assert_eq!(creds.client_id, "client-1");
    assert_eq!(creds.granted_scopes, vec!["read"]);
    assert_eq!(creds.token_received_at, Some(1000));
}

#[test]
#[cfg(unix)]
fn save_sets_0600_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let (store, dir) = make_store();
    let path = dir.path().join("mcp-auth.json");

    store.save_to(&path).expect("save");

    let metadata = std::fs::metadata(&path).expect("metadata");
    let mode = metadata.permissions().mode();
    // 0600 = owner read+write only
    assert_eq!(mode & 0o777, 0o600, "expected 0600 permissions");
}

#[test]
fn remove_clears_entry() {
    let (store, dir) = make_store();
    let path = dir.path().join("mcp-auth.json");

    // Save with entry, then remove, then save again
    store.save_to(&path).expect("save");
    let mut store2 = McpCredentialsStore::load_from(&path).expect("reload");
    store2.remove("server-a");
    store2.save_to(&path).expect("save again");

    let loaded = McpCredentialsStore::load_from(&path).expect("final load");
    assert!(loaded.entries.is_empty(), "entry should be removed");
}

#[test]
fn corrupt_json_returns_empty_store() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("corrupt.json");

    let mut file = std::fs::File::create(&path).expect("create file");
    file.write_all(b"this is not valid json{broken")
        .expect("write");

    let store = McpCredentialsStore::load_from(&path).expect("load corrupt");
    assert!(store.entries.is_empty(), "corrupt file -> empty store");
}

#[test]
fn concurrent_saves_do_not_corrupt_file() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("concurrent.json");

    // Save from two threads — file locking should prevent corruption
    let path1 = path.clone();
    let path2 = path.clone();
    let jh1 = std::thread::spawn(move || {
        for i in 0..20 {
            let mut s = McpCredentialsStore::default();
            let entry = s.entries.entry(format!("t1-{i}")).or_default();
            entry.stored_credentials = Some(StoredCredentials::new(
                format!("at1-{i}"),
                None,
                vec![],
                None,
            ));
            s.save_to(&path1).ok();
        }
    });
    let jh2 = std::thread::spawn(move || {
        for i in 0..20 {
            let mut s = McpCredentialsStore::default();
            let entry = s.entries.entry(format!("t2-{i}")).or_default();
            entry.stored_credentials = Some(StoredCredentials::new(
                format!("at2-{i}"),
                None,
                vec![],
                None,
            ));
            s.save_to(&path2).ok();
        }
    });

    jh1.join().expect("thread 1");
    jh2.join().expect("thread 2");

    // File should be valid JSON
    let loaded = McpCredentialsStore::load_from(&path).expect("load after concurrent saves");
    // At least one entry should exist
    assert!(
        !loaded.entries.is_empty(),
        "concurrent saves should produce valid output"
    );
}

#[test]
fn load_from_path_io_error_propagates() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("unreadable.json");

    // Create a directory at the path so reading it as a file fails with an I/O error
    // (not NotFound, which is handled gracefully)
    std::fs::create_dir(&path).expect("create dir at path");

    let result = McpCredentialsStore::load_from(&path);
    match result {
        Err(CredentialsError::Io(_)) => {} // expected
        other => panic!("expected Io error, got: {other:?}"),
    }
}

#[test]
fn default_path_uses_xdg_data_home() {
    // Just verify the path ends with the expected components
    let path = McpCredentialsStore::default_path().expect("default path");
    let path_str = path.to_string_lossy();
    assert!(
        path_str.contains("nu-agent"),
        "path should contain nu-agent: {path_str}"
    );
    assert!(
        path_str.contains("mcp-auth.json"),
        "path should end with mcp-auth.json: {path_str}"
    );
}

#[cfg(unix)]
#[test]
fn save_tightens_permissions_on_existing_file() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mcp-auth.json");

    // Create a file with loose permissions (0644)
    std::fs::write(&path, "{}").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    // Verify starting permissions are 0644
    let perms = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(perms & 0o777, 0o644);

    // Save to the existing file
    let store = McpCredentialsStore::default();
    store.save_to(&path).unwrap();

    // Verify permissions are now 0600
    let perms = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(perms & 0o777, 0o600);
}

// ---------------------------------------------------------------------------
// FileCredentialStore tests
// ---------------------------------------------------------------------------

fn make_shared_store() -> (Arc<Mutex<McpCredentialsStore>>, TempDir, PathBuf) {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("mcp-auth.json");
    let store = McpCredentialsStore::default();
    store.save_to(&path).expect("save empty store");
    let mut loaded = McpCredentialsStore::load_from(&path).expect("load");
    let entry = loaded.entries.entry("server-a".to_string()).or_default();
    entry.server_url = Some("https://example.com/mcp".to_string());
    entry.stored_credentials = Some(StoredCredentials::new(
        "client-1".to_string(),
        None,
        vec!["read".to_string()],
        Some(1000),
    ));
    loaded.save_to(&path).expect("save with entry");
    (Arc::new(Mutex::new(loaded)), dir, path)
}

#[tokio::test]
async fn file_credential_store_load_returns_none_for_unknown_server() {
    let (store, _dir, path) = make_shared_store();
    let cred_store = FileCredentialStore::new_with_path(store, "unknown-server", path);
    let result = cred_store.load().await.expect("load");
    assert!(result.is_none(), "unknown server should return None");
}

#[tokio::test]
async fn file_credential_store_save_then_load_round_trip() {
    let (store, _dir, path) = make_shared_store();
    let cred_store = FileCredentialStore::new_with_path(store.clone(), "server-a", path);

    let creds = StoredCredentials::new(
        "client-1".to_string(),
        None,
        vec!["read".to_string()],
        Some(1000),
    );
    cred_store.save(creds.clone()).await.expect("save");

    let loaded = cred_store.load().await.expect("load");
    let stored = loaded.expect("should have credentials");
    assert_eq!(stored.client_id, "client-1");
    assert_eq!(stored.granted_scopes, vec!["read"]);
    assert_eq!(stored.token_received_at, Some(1000));
}

#[tokio::test]
async fn file_credential_store_clear_removes_credentials() {
    let (store, _dir, path) = make_shared_store();
    let cred_store = FileCredentialStore::new_with_path(store.clone(), "server-a", path);

    let creds = StoredCredentials::new("client-1".to_string(), None, vec![], None);
    cred_store.save(creds).await.expect("save");
    cred_store.clear().await.expect("clear");

    let loaded = cred_store.load().await.expect("load");
    assert!(loaded.is_none(), "credentials should be cleared");
}

#[tokio::test]
async fn file_credential_store_multiple_servers_independent() {
    let (store, _dir, path) = make_shared_store();
    let store_a = FileCredentialStore::new_with_path(store.clone(), "server-a", path.clone());
    let store_b = FileCredentialStore::new_with_path(store.clone(), "server-b", path);

    let creds_a = StoredCredentials::new("client-a".to_string(), None, vec![], None);
    let creds_b = StoredCredentials::new("client-b".to_string(), None, vec![], None);

    store_a.save(creds_a).await.expect("save a");
    store_b.save(creds_b).await.expect("save b");

    let loaded_a = store_a
        .load()
        .await
        .expect("load a")
        .expect("should have a");
    let loaded_b = store_b
        .load()
        .await
        .expect("load b")
        .expect("should have b");
    assert_eq!(loaded_a.client_id, "client-a");
    assert_eq!(loaded_b.client_id, "client-b");
}

// ---------------------------------------------------------------------------
// FileStateStore tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn file_state_store_save_then_load_round_trip() {
    let (store, _dir, path) = make_shared_store();
    let state_store = FileStateStore::new_with_path(store.clone(), path);

    let state = StoredAuthorizationState::new(
        &PkceCodeVerifier::new("pkce-verifier-123".to_string()),
        &CsrfToken::new("csrf-token-456".to_string()),
    );
    state_store
        .save("csrf-token-456", state.clone())
        .await
        .expect("save");

    let loaded = state_store.load("csrf-token-456").await.expect("load");
    let stored = loaded.expect("should have state");
    assert_eq!(stored.pkce_verifier, "pkce-verifier-123");
    assert_eq!(stored.csrf_token, "csrf-token-456");
}

#[tokio::test]
async fn file_state_store_load_returns_none_for_unknown_token() {
    let (store, _dir, path) = make_shared_store();
    let state_store = FileStateStore::new_with_path(store, path);
    let result = state_store.load("unknown-token").await.expect("load");
    assert!(result.is_none(), "unknown token should return None");
}

#[tokio::test]
async fn file_state_store_delete_removes_state() {
    let (store, _dir, path) = make_shared_store();
    let state_store = FileStateStore::new_with_path(store.clone(), path);

    let state = StoredAuthorizationState::new(
        &PkceCodeVerifier::new("pkce-verifier-123".to_string()),
        &CsrfToken::new("csrf-token-456".to_string()),
    );
    state_store
        .save("csrf-token-456", state)
        .await
        .expect("save");
    state_store.delete("csrf-token-456").await.expect("delete");

    let loaded = state_store.load("csrf-token-456").await.expect("load");
    assert!(loaded.is_none(), "state should be deleted");
}

#[tokio::test]
async fn file_state_store_multiple_tokens_independent() {
    let (store, _dir, path) = make_shared_store();
    let state_store = FileStateStore::new_with_path(store.clone(), path);

    let state_a = StoredAuthorizationState::new(
        &PkceCodeVerifier::new("verifier-a".to_string()),
        &CsrfToken::new("token-a".to_string()),
    );
    let state_b = StoredAuthorizationState::new(
        &PkceCodeVerifier::new("verifier-b".to_string()),
        &CsrfToken::new("token-b".to_string()),
    );

    state_store.save("token-a", state_a).await.expect("save a");
    state_store.save("token-b", state_b).await.expect("save b");

    let loaded_a = state_store
        .load("token-a")
        .await
        .expect("load a")
        .expect("should have a");
    let loaded_b = state_store
        .load("token-b")
        .await
        .expect("load b")
        .expect("should have b");
    assert_eq!(loaded_a.pkce_verifier, "verifier-a");
    assert_eq!(loaded_b.pkce_verifier, "verifier-b");
}

#[tokio::test]
async fn shared_store_credential_and_state_consistency() {
    let (store, _dir, path) = make_shared_store();
    let cred_store = FileCredentialStore::new_with_path(store.clone(), "server-a", path.clone());
    let state_store = FileStateStore::new_with_path(store.clone(), path);

    // Save credentials
    let creds = StoredCredentials::new("client-1".to_string(), None, vec![], None);
    cred_store.save(creds).await.expect("save creds");

    // Save state
    let state = StoredAuthorizationState::new(
        &PkceCodeVerifier::new("pkce-verifier".to_string()),
        &CsrfToken::new("csrf-token".to_string()),
    );
    state_store
        .save("csrf-token", state)
        .await
        .expect("save state");

    // Both should be visible through the shared store
    let guard = store.lock().await;
    assert!(
        guard.entries.contains_key("server-a"),
        "credential entry should exist"
    );
    assert!(
        guard.oauth_states.contains_key("csrf-token"),
        "oauth state should exist"
    );
}

#[test]
fn mcp_credentials_entry_debug_redacts_sensitive_fields() {
    use super::McpCredentialsEntry;

    let entry = McpCredentialsEntry {
        server_url: Some("https://example.com/mcp".to_string()),
        stored_credentials: Some(StoredCredentials::new(
            "my-client".to_string(),
            None,
            vec!["read".to_string()],
            Some(1000),
        )),
    };

    let debug_output = format!("{:?}", entry);

    // Redaction markers should be present
    assert!(
        debug_output.contains("[REDACTED]"),
        "debug output should contain [REDACTED]"
    );

    // Non-sensitive fields should still be visible
    assert!(
        debug_output.contains("https://example.com/mcp"),
        "server_url should be visible"
    );
}
