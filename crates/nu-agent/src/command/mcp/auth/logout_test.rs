use std::collections::HashMap;

use nu_agent_core::tools::mcp::credentials::McpCredentialsStore;

use super::logout::perform_logout;

fn make_store_with_entry(server_name: &str) -> McpCredentialsStore {
    let mut store = McpCredentialsStore::default();
    store
        .entries
        .insert(server_name.to_string(), Default::default());
    store
}

#[test]
fn logout_removes_entry_from_store() {
    let mut store = make_store_with_entry("my-server");
    assert!(store.entries.contains_key("my-server"));

    let msg = perform_logout(&mut store, "my-server");

    assert!(!store.entries.contains_key("my-server"));
    assert_eq!(msg, "Cleared credentials for 'my-server'");
}

#[test]
fn logout_on_nonexistent_server_returns_no_credentials_message() {
    let mut store = McpCredentialsStore::default();

    let msg = perform_logout(&mut store, "unknown-server");

    assert!(store.entries.is_empty());
    assert_eq!(msg, "No stored credentials for 'unknown-server'");
}

#[test]
fn logout_persists_to_disk() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("mcp-auth.json");

    // Create a store with an entry and save it
    let store = make_store_with_entry("my-server");
    store.save_to(&path).expect("save");

    // Reload, perform logout, save
    let mut loaded = McpCredentialsStore::load_from(&path).expect("load");
    assert!(loaded.entries.contains_key("my-server"));

    let _msg = perform_logout(&mut loaded, "my-server");
    loaded.save_to(&path).expect("save");

    // Reload again and verify entry is gone
    let final_store = McpCredentialsStore::load_from(&path).expect("load");
    assert!(!final_store.entries.contains_key("my-server"));
    assert!(final_store.entries.is_empty());
}

#[test]
fn logout_removes_only_target_server() {
    let mut store = McpCredentialsStore {
        entries: HashMap::from([
            ("server-a".to_string(), Default::default()),
            ("server-b".to_string(), Default::default()),
            ("server-c".to_string(), Default::default()),
        ]),
        ..Default::default()
    };

    let msg = perform_logout(&mut store, "server-b");

    assert_eq!(store.entries.len(), 2);
    assert!(store.entries.contains_key("server-a"));
    assert!(!store.entries.contains_key("server-b"));
    assert!(store.entries.contains_key("server-c"));
    assert_eq!(msg, "Cleared credentials for 'server-b'");
}
