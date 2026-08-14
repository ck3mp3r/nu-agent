use super::factory::{SessionStoreImpl, StoreType, create_store};
use crate::session::SessionStore;
use crate::types::Message;
use serial_test::serial;

fn make_test_message(text: &str) -> Message {
    Message::user(text)
}

#[test]
fn store_type_from_str_parses_memory() {
    assert_eq!("memory".parse::<StoreType>().unwrap(), StoreType::Memory);
}

#[test]
fn store_type_from_str_parses_case_insensitive() {
    assert_eq!("MEMORY".parse::<StoreType>().unwrap(), StoreType::Memory);
    assert_eq!("Memory".parse::<StoreType>().unwrap(), StoreType::Memory);
}

#[test]
fn store_type_from_str_error_mentions_memory() {
    let err = "unknown".parse::<StoreType>().unwrap_err();
    assert!(
        err.contains("memory"),
        "error should mention 'memory': {err}"
    );
}

#[tokio::test]
async fn create_store_memory_returns_sqlite_impl() {
    let store = create_store(StoreType::Memory)
        .await
        .expect("create memory store");
    assert!(matches!(store, SessionStoreImpl::Sqlite(_)));
}

#[tokio::test]
async fn create_store_memory_creates_working_store() {
    let store = create_store(StoreType::Memory)
        .await
        .expect("create memory store");
    let test_id = "test-memory-session";
    let messages = vec![make_test_message("hello")];

    store
        .create(test_id, &messages)
        .await
        .expect("create session");
    let loaded = store.load(test_id).await.expect("load session");
    assert!(loaded.is_some(), "session should exist");
    let (_metadata, entries) = loaded.unwrap();
    assert_eq!(entries.len(), 1);

    let listed = store.list().await.expect("list sessions");
    assert!(
        listed.iter().any(|s| s.id == test_id),
        "session should appear in list"
    );

    store.delete(test_id).await.expect("delete session");
    let loaded_after_delete = store.load(test_id).await.expect("load after delete");
    assert!(loaded_after_delete.is_none(), "session should be gone");
}

#[tokio::test]
#[serial]
async fn create_store_memory_no_disk_io() {
    use tempfile::TempDir;
    let temp = TempDir::new().unwrap();
    let original_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(temp.path()).unwrap();

    let store = create_store(StoreType::Memory)
        .await
        .expect("create memory store");
    let messages = vec![make_test_message("test")];
    store
        .create("ephemeral-test", &messages)
        .await
        .expect("create");

    // Verify no database file was created in the temp directory
    let entries: Vec<_> = std::fs::read_dir(temp.path()).unwrap().collect();
    assert!(
        entries.is_empty(),
        "temp dir should be empty — in-memory store should not write to disk. Found: {:?}",
        entries
            .iter()
            .map(|e| e.as_ref().map(|e| e.path()).unwrap_or_default())
            .collect::<Vec<_>>()
    );

    std::env::set_current_dir(original_cwd).unwrap();
}
