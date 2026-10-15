use super::*;

// ================================================================
// Edge cases
// ================================================================

#[tokio::test]
async fn in_memory_sqlite_works() {
    // Verify that :memory: SQLite works for tests
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create in-memory store");

    // Basic operations should work
    store
        .create("mem-test", &[Message::user("hello")])
        .await
        .expect("create in memory");

    let result = store.load("mem-test").await.expect("load");
    assert!(result.is_some(), "should load session from in-memory db");
}

#[tokio::test]
async fn corrupt_row_skipped_not_fatal() -> Result<()> {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");
    let session_id = "corrupt-test";

    // Create a session with valid messages
    store
        .create(session_id, &[Message::user("valid")])
        .await
        .expect("create");

    // Directly insert a corrupt row into the entries table
    sqlx::query("INSERT INTO entries (session_id, seq, entry_type, data) VALUES (?, ?, ?, ?)")
        .bind(session_id)
        .bind(1i64)
        .bind("message")
        .bind("this is not valid json")
        .execute(&store.pool)
        .await
        .expect("insert corrupt row");

    // Insert a valid message after the corrupt one
    let valid_json = serde_json::to_string(&Message::assistant("after corrupt")).unwrap();
    sqlx::query("INSERT INTO entries (session_id, seq, entry_type, data) VALUES (?, ?, ?, ?)")
        .bind(session_id)
        .bind(2i64)
        .bind("message")
        .bind(&valid_json)
        .execute(&store.pool)
        .await
        .expect("insert valid row after corrupt");

    // Load should skip the corrupt row and return valid entries
    let (_metadata, entries) = store
        .load(session_id)
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    // Should have 2 entries: the original valid message + the valid message after corrupt
    assert_eq!(entries.len(), 2, "corrupt row should be skipped");

    // Verify the valid entries are correct
    match &entries[0] {
        StoreEntry::Message(m) => {
            let json = serde_json::to_value(m).unwrap();
            assert!(json.to_string().contains("valid"));
        }
        _ => panic!("Expected Message entry at index 0"),
    }
    match &entries[1] {
        StoreEntry::Message(m) => {
            let json = serde_json::to_value(m).unwrap();
            assert!(json.to_string().contains("after corrupt"));
        }
        _ => panic!("Expected Message entry at index 1"),
    }
    Ok(())
}
