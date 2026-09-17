use crate::config::vault::VaultBackend;

use super::{INDEX_KEY, PROBE_KEY, SERVICE_NAME};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_keychain_backend_constants_match_key_naming_convention() -> Result<()> {
    // -- Setup & Fixtures
    // (no fixtures — the constants are compile-time values)

    // -- Exec
    let service = SERVICE_NAME;
    let index_key = INDEX_KEY;
    let probe_key = PROBE_KEY;

    // -- Check
    assert_eq!(service, "nu-agent", "service name is fixed");
    assert_eq!(
        index_key, "nu-agent:index",
        "index key follows the convention"
    );
    assert_eq!(probe_key, "__vault_probe__", "probe key is fixed");
    Ok(())
}

#[test]
fn test_keychain_backend_index_serializes_as_json_array() -> Result<()> {
    // -- Setup & Fixtures
    let keys = vec![
        "provider:openai".to_string(),
        "mcp:cred:my-server".to_string(),
    ];

    // -- Exec
    let raw = serde_json::to_string(&keys)?;
    let round_trip: Vec<String> = serde_json::from_str(&raw)?;

    // -- Check
    assert_eq!(raw, r#"["provider:openai","mcp:cred:my-server"]"#);
    assert_eq!(round_trip, keys);
    Ok(())
}

#[test]
fn test_keychain_backend_index_add_appends_once() -> Result<()> {
    // -- Setup & Fixtures
    let start = vec!["provider:openai".to_string()];

    // -- Exec
    let appended = super::added(&start, "mcp:bearer:server");
    let idempotent = super::added(&appended, "mcp:bearer:server");

    // -- Check
    assert_eq!(
        appended,
        vec![
            "provider:openai".to_string(),
            "mcp:bearer:server".to_string()
        ],
        "add appends a new key"
    );
    assert_eq!(idempotent.len(), 2, "add is idempotent for an existing key");
    Ok(())
}

#[test]
fn test_keychain_backend_index_remove_drops_key() -> Result<()> {
    // -- Setup & Fixtures
    let start = vec![
        "provider:openai".to_string(),
        "mcp:bearer:server".to_string(),
    ];

    // -- Exec
    let without = super::removed(&start, "provider:openai");
    let unchanged = super::removed(&without, "provider:openai");

    // -- Check
    assert_eq!(without, vec!["mcp:bearer:server".to_string()]);
    assert_eq!(unchanged, without, "remove is idempotent for a missing key");
    Ok(())
}

// The remaining tests touch the real OS keychain. They are ignored because
// CI has no credential store. Run them on a desktop with:
//   cargo test -p nu-agent-core keychain_backend -- --ignored

#[test]
#[ignore = "requires a real OS keychain"]
fn test_keychain_backend_probe_succeeds_with_keychain() -> Result<()> {
    // -- Setup & Fixtures
    // (no fixtures — probe discovers the credential store)

    // -- Exec
    let probed = super::KeychainBackend::probe();

    // -- Check
    let _backend = probed.map_err(|e| format!("probe should find a keychain: {e}"))?;
    Ok(())
}

#[test]
#[ignore = "requires a real OS keychain"]
fn test_keychain_backend_set_get_round_trip() -> Result<()> {
    // -- Setup & Fixtures
    let backend = super::KeychainBackend;
    let key = "provider:task2-test-round-trip";

    // -- Exec
    backend.set(key, "sk-test")?;
    let fetched = backend.get(key)?;

    // -- Check
    assert_eq!(fetched.as_deref(), Some("sk-test"));

    // -- Cleanup
    backend.delete(key)?;
    Ok(())
}

#[test]
#[ignore = "requires a real OS keychain"]
fn test_keychain_backend_get_missing_key_returns_none() -> Result<()> {
    // -- Setup & Fixtures
    let backend = super::KeychainBackend;

    // -- Exec
    let fetched = backend.get("provider:task2-test-definitely-missing")?;

    // -- Check
    assert_eq!(fetched, None);
    Ok(())
}

#[test]
#[ignore = "requires a real OS keychain"]
fn test_keychain_backend_delete_is_idempotent() -> Result<()> {
    // -- Setup & Fixtures
    let backend = super::KeychainBackend;
    let key = "provider:task2-test-idempotent-delete";
    backend.set(key, "sk-delete-me")?;

    // -- Exec
    backend.delete(key)?;
    let after_first = backend.get(key)?;
    let second_delete = backend.delete(key);

    // -- Check
    assert_eq!(after_first, None, "deleted key is gone");
    assert!(second_delete.is_ok(), "deleting twice is not an error");
    Ok(())
}

#[test]
#[ignore = "requires a real OS keychain"]
fn test_keychain_backend_list_contains_set_keys() -> Result<()> {
    // -- Setup & Fixtures
    let backend = super::KeychainBackend;
    let key1 = "provider:task2-test-list-a";
    let key2 = "provider:task2-test-list-b";

    // -- Exec
    backend.set(key1, "val1")?;
    backend.set(key2, "val2")?;
    let listed = backend.list()?;
    backend.delete(key1)?;
    let after_delete = backend.list()?;

    // -- Cleanup
    backend.delete(key2)?;

    // -- Check
    assert!(listed.contains(&key1.to_string()), "list contains key1");
    assert!(listed.contains(&key2.to_string()), "list contains key2");
    assert!(
        !after_delete.contains(&key1.to_string()),
        "list drops key1 after delete"
    );
    Ok(())
}

// endregion: --- Tests
