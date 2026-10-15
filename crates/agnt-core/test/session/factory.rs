use super::factory::{SessionStoreBackend, StoreType, create_store, create_store_in};
use crate::session::SessionStore;
use crate::types::Message;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn make_test_message(text: &str) -> Message {
    Message::user(text)
}

/// `StoreType` parses case-insensitively and its error names the valid values.
#[test]
fn store_type_from_str() -> Result<()> {
    // -- Case 1: lowercase parses
    assert_eq!("memory".parse::<StoreType>()?, StoreType::Memory);

    // -- Case 2: parsing is case-insensitive
    assert_eq!("MEMORY".parse::<StoreType>()?, StoreType::Memory);
    assert_eq!("Memory".parse::<StoreType>()?, StoreType::Memory);

    // -- Case 3: the error mentions the valid values
    let err = "unknown".parse::<StoreType>().unwrap_err();
    assert!(
        err.contains("memory"),
        "error should mention 'memory': {err}"
    );
    Ok(())
}

/// `create_store(StoreType::Memory)` returns the SQLite backend backed by an
/// in-memory database that supports the full store API without disk I/O.
#[tokio::test]
async fn create_store_memory() -> Result<()> {
    // -- Case 1: the memory store uses the SQLite backend
    let store = create_store(StoreType::Memory).await?;
    assert!(matches!(store, SessionStoreBackend::Sqlite(_)));

    // -- Case 2: the store supports create/load/list/delete
    let test_id = "test-memory-session";
    let messages = vec![make_test_message("hello")];
    store.create(test_id, &messages).await?;
    let loaded = store.load(test_id).await?;
    let (_metadata, entries) = loaded.ok_or("session should exist")?;
    assert_eq!(entries.len(), 1);

    let listed = store.list().await?;
    assert!(
        listed.iter().any(|s| s.id == test_id),
        "session should appear in list"
    );

    store.delete(test_id).await?;
    assert!(
        store.load(test_id).await?.is_none(),
        "session should be gone"
    );

    // -- Case 3: no database file is written to disk
    use tempfile::TempDir;
    let temp = TempDir::new()?;
    let store = create_store_in(StoreType::Memory, temp.path()).await?;
    store
        .create("ephemeral-test", &[make_test_message("test")])
        .await?;

    let entries: Vec<_> = std::fs::read_dir(temp.path())?.collect();
    assert!(
        entries.is_empty(),
        "temp dir should be empty — in-memory store should not write to disk. Found: {:?}",
        entries
            .iter()
            .map(|e| e.as_ref().map(|e| e.path()).unwrap_or_default())
            .collect::<Vec<_>>()
    );
    Ok(())
}
