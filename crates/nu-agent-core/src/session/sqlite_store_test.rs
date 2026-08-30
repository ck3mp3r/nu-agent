use super::SqliteSessionStore;
use crate::session::store::{CompactionMarker, SessionStore, StoreEntry};
use crate::types::Message;
use chrono::Utc;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ================================================================
// Basic CRUD
// ================================================================

#[tokio::test]
async fn create_and_load_round_trip() -> Result<()> {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");
    let session_id = "test-session-1";
    let messages = vec![Message::user("Hello"), Message::assistant("Hi there!")];

    store.create(session_id, &messages).await.expect("create");

    let (metadata, entries) = store
        .load(session_id)
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(metadata.session_id, session_id);
    assert_eq!(entries.len(), 2);

    // Verify message content
    match &entries[0] {
        StoreEntry::Message(m) => {
            let json = serde_json::to_value(m).unwrap();
            assert_eq!(json["role"], "user");
            assert!(json.to_string().contains("Hello"));
        }
        _ => panic!("Expected Message entry at index 0"),
    }
    match &entries[1] {
        StoreEntry::Message(m) => {
            let json = serde_json::to_value(m).unwrap();
            assert_eq!(json["role"], "assistant");
            assert!(json.to_string().contains("Hi there!"));
        }
        _ => panic!("Expected Message entry at index 1"),
    }
    Ok(())
}

#[tokio::test]
async fn create_and_load_with_markers() -> Result<()> {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");
    let session_id = "test-session-markers";

    // Create session with messages
    store
        .create(session_id, &[Message::user("Hello")])
        .await
        .expect("create");

    // Append a compaction marker
    let marker = CompactionMarker::new("Summary of old messages".to_string(), Utc::now());
    store
        .append(session_id, &[StoreEntry::Marker(marker)])
        .await
        .expect("append marker");

    // Append more messages after marker
    store
        .append(
            session_id,
            &[StoreEntry::Message(Message::assistant(
                "Post-compaction reply",
            ))],
        )
        .await
        .expect("append post-marker message");

    // Load and verify all entries preserved
    let (_metadata, entries) = store
        .load(session_id)
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), 3, "should have 1 msg + 1 marker + 1 msg");

    assert!(
        matches!(&entries[0], StoreEntry::Message(_)),
        "entry 0 should be a message"
    );
    assert!(
        matches!(&entries[1], StoreEntry::Marker(m) if m.summary == "Summary of old messages"),
        "entry 1 should be the compaction marker"
    );
    assert!(
        matches!(&entries[2], StoreEntry::Message(_)),
        "entry 2 should be a message"
    );
    Ok(())
}

#[tokio::test]
async fn load_nonexistent_returns_none() {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    let result = store.load("i-do-not-exist").await.expect("load");
    assert!(result.is_none(), "non-existent session should return None");
}

#[tokio::test]
async fn load_empty_session_returns_none() {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    // Create a session row with no entries by appending to a non-existent session
    // (append creates the session row but we pass an empty entries slice)
    store
        .append("empty-session", &[])
        .await
        .expect("append empty entries");

    // Load should return None because there are zero entries
    let result = store.load("empty-session").await.expect("load");
    assert!(
        result.is_none(),
        "session with zero entries should return None"
    );
}

// ================================================================
// Append
// ================================================================

#[tokio::test]
async fn append_extends_session() -> Result<()> {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");
    let session_id = "append-test";

    // Create with initial messages
    store
        .create(session_id, &[Message::user("First")])
        .await
        .expect("create");

    // Append more entries
    store
        .append(
            session_id,
            &[StoreEntry::Message(Message::assistant("Response"))],
        )
        .await
        .expect("append");

    // Load and verify all entries present
    let (_metadata, entries) = store
        .load(session_id)
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), 2, "should have 2 entries after append");

    // Verify order
    match &entries[0] {
        StoreEntry::Message(m) => {
            let json = serde_json::to_value(m).unwrap();
            assert!(json.to_string().contains("First"));
        }
        _ => panic!("Expected Message entry at index 0"),
    }
    match &entries[1] {
        StoreEntry::Message(m) => {
            let json = serde_json::to_value(m).unwrap();
            assert!(json.to_string().contains("Response"));
        }
        _ => panic!("Expected Message entry at index 1"),
    }
    Ok(())
}

#[tokio::test]
async fn append_position_order_preserved() -> Result<()> {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");
    let session_id = "append-order";

    // Create with 2 messages
    store
        .create(session_id, &[Message::user("A"), Message::assistant("B")])
        .await
        .expect("create");

    // Append 2 more
    store
        .append(
            session_id,
            &[
                StoreEntry::Message(Message::user("C")),
                StoreEntry::Message(Message::assistant("D")),
            ],
        )
        .await
        .expect("append 1");

    // Append 1 more
    store
        .append(session_id, &[StoreEntry::Message(Message::user("E"))])
        .await
        .expect("append 2");

    // Load and verify sequential order
    let (_metadata, entries) = store
        .load(session_id)
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), 5, "should have 5 entries total");

    // Verify each entry's text content via JSON
    let expected_texts = ["A", "B", "C", "D", "E"];
    for (i, entry) in entries.iter().enumerate() {
        match entry {
            StoreEntry::Message(m) => {
                let json = serde_json::to_value(m).unwrap();
                assert!(
                    json.to_string().contains(expected_texts[i]),
                    "entry {i} should contain '{}'",
                    expected_texts[i]
                );
            }
            _ => panic!("Expected Message entry at index {i}"),
        }
    }
    Ok(())
}

#[tokio::test]
async fn append_to_nonexistent_session() -> Result<()> {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    // Append to a session that doesn't exist yet — should create it
    store
        .append(
            "brand-new-session",
            &[StoreEntry::Message(Message::user(
                "First message in new session",
            ))],
        )
        .await
        .expect("append to non-existent session");

    // Load and verify
    let (_metadata, entries) = store
        .load("brand-new-session")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), 1);
    match &entries[0] {
        StoreEntry::Message(m) => {
            let json = serde_json::to_value(m).unwrap();
            assert!(json.to_string().contains("First message in new session"));
        }
        _ => panic!("Expected Message entry"),
    }
    Ok(())
}

// ================================================================
// Replace entries (compaction)
// ================================================================

#[tokio::test]
async fn replace_entries_cleans_and_rewrites() -> Result<()> {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");
    let session_id = "replace-test";

    // Create with 10 entries
    let mut messages = Vec::new();
    for i in 0..5 {
        messages.push(Message::user(format!("user_msg_{}", i)));
        messages.push(Message::assistant(format!("assistant_msg_{}", i)));
    }
    store.create(session_id, &messages).await.expect("create");

    // Replace with 3 entries
    let new_entries = vec![
        StoreEntry::Message(Message::user("compacted_user")),
        StoreEntry::Message(Message::assistant("compacted_assistant")),
        StoreEntry::Marker(CompactionMarker::new(
            "Compacted summary".to_string(),
            Utc::now(),
        )),
    ];
    store
        .replace_entries(session_id, &new_entries)
        .await
        .expect("replace entries");

    // Load and verify only 3 remain
    let (_metadata, entries) = store
        .load(session_id)
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(
        entries.len(),
        3,
        "should have exactly 3 entries after replace"
    );

    // Verify content
    match &entries[0] {
        StoreEntry::Message(m) => {
            let json = serde_json::to_value(m).unwrap();
            assert!(json.to_string().contains("compacted_user"));
        }
        _ => panic!("Expected Message entry at index 0"),
    }
    match &entries[2] {
        StoreEntry::Marker(m) => {
            assert_eq!(m.summary, "Compacted summary");
        }
        _ => panic!("Expected Marker entry at index 2"),
    }
    Ok(())
}

#[tokio::test]
async fn replace_entries_preserves_order() -> Result<()> {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");
    let session_id = "replace-order";

    store
        .create(session_id, &[Message::user("original")])
        .await
        .expect("create");

    // Replace with entries in specific order
    let new_entries = vec![
        StoreEntry::Message(Message::user("first")),
        StoreEntry::Marker(CompactionMarker::new("Marker".to_string(), Utc::now())),
        StoreEntry::Message(Message::assistant("second")),
    ];
    store
        .replace_entries(session_id, &new_entries)
        .await
        .expect("replace entries");

    // Load and verify order
    let (_metadata, entries) = store
        .load(session_id)
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), 3);

    assert!(
        matches!(&entries[0], StoreEntry::Message(_)),
        "entry 0 should be a message"
    );
    assert!(
        matches!(&entries[1], StoreEntry::Marker(m) if m.summary == "Marker"),
        "entry 1 should be the marker"
    );
    assert!(
        matches!(&entries[2], StoreEntry::Message(_)),
        "entry 2 should be a message"
    );
    Ok(())
}

// ================================================================
// List
// ================================================================

#[tokio::test]
async fn list_returns_newest_first() {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    // Create 3 sessions with different timestamps
    // We create them in order and rely on created_at timestamps
    store
        .create("session-oldest", &[Message::user("oldest")])
        .await
        .expect("create oldest");

    // Small delay to ensure different timestamps
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    store
        .create("session-middle", &[Message::user("middle")])
        .await
        .expect("create middle");

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    store
        .create("session-newest", &[Message::user("newest")])
        .await
        .expect("create newest");

    let sessions = store.list().await.expect("list");
    assert_eq!(sessions.len(), 3, "should list all 3 sessions");

    // Newest first
    assert_eq!(
        sessions[0].id, "session-newest",
        "newest session should be first"
    );
    assert_eq!(
        sessions[1].id, "session-middle",
        "middle session should be second"
    );
    assert_eq!(
        sessions[2].id, "session-oldest",
        "oldest session should be last"
    );
}

#[tokio::test]
async fn list_filters_zero_entry_sessions() {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    // Create a session with entries
    store
        .create("session-with-data", &[Message::user("data")])
        .await
        .expect("create with data");

    // Create a session row with no entries by appending empty
    store
        .append("session-empty", &[])
        .await
        .expect("append empty");

    // List should only include the session with entries
    let sessions = store.list().await.expect("list");
    assert_eq!(sessions.len(), 1, "should filter out zero-entry sessions");
    assert_eq!(sessions[0].id, "session-with-data");
}

// ================================================================
// Delete
// ================================================================

#[tokio::test]
async fn delete_removes_session_and_entries() {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    store
        .create("delete-me", &[Message::user("bye")])
        .await
        .expect("create");

    // Verify it exists
    assert!(
        store.load("delete-me").await.expect("load").is_some(),
        "session should exist before delete"
    );

    // Delete
    store.delete("delete-me").await.expect("delete");

    // Verify it's gone
    assert!(
        store.load("delete-me").await.expect("load").is_none(),
        "session should not exist after delete"
    );

    // List should be empty
    let sessions = store.list().await.expect("list");
    assert_eq!(sessions.len(), 0, "list should be empty after delete");
}

#[tokio::test]
async fn delete_nonexistent_is_noop() {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    // Deleting a non-existent session should not error
    let result = store.delete("i-never-existed").await;
    assert!(
        result.is_ok(),
        "deleting non-existent session should be Ok(())"
    );
}

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

// ================================================================
// Title extraction tests
// ================================================================

#[tokio::test]
async fn title_extracted_from_first_user_message_sqlite() -> Result<()> {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    let messages = vec![
        Message::user("Hello, this is my first message"),
        Message::assistant("Hi there!"),
    ];
    store
        .create("title-test-1", &messages)
        .await
        .expect("create");

    // Verify via list
    let sessions = store.list().await.expect("list");
    let session = sessions.iter().find(|s| s.id == "title-test-1").unwrap();
    assert_eq!(
        session.title,
        Some("Hello, this is my first message".to_string())
    );

    // Verify via load
    let (metadata, _entries) = store
        .load("title-test-1")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(
        metadata.title,
        Some("Hello, this is my first message".to_string())
    );
    Ok(())
}

#[tokio::test]
async fn title_none_when_no_user_message_sqlite() -> Result<()> {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    let messages = vec![Message::assistant("Hello!")];
    store
        .create("no-user-msg-sqlite", &messages)
        .await
        .expect("create");

    let sessions = store.list().await.expect("list");
    let session = sessions
        .iter()
        .find(|s| s.id == "no-user-msg-sqlite")
        .unwrap();
    assert_eq!(session.title, None);

    let (metadata, _entries) = store
        .load("no-user-msg-sqlite")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(metadata.title, None);
    Ok(())
}

#[tokio::test]
async fn title_survives_round_trip_sqlite() -> Result<()> {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    let messages = vec![
        Message::user("Round trip title test"),
        Message::assistant("Response"),
    ];
    store.create("round-trip", &messages).await.expect("create");

    // list → load
    let sessions = store.list().await.expect("list");
    let session = sessions.iter().find(|s| s.id == "round-trip").unwrap();
    assert_eq!(session.title, Some("Round trip title test".to_string()));

    let (metadata, _entries) = store
        .load("round-trip")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(metadata.title, Some("Round trip title test".to_string()));
    Ok(())
}

// ================================================================
// Pool connection persistence
// ================================================================

#[tokio::test]
async fn memory_store_persists_sessions_table_across_queries() {
    // Regression test: the :memory: SQLite database is per-connection.
    // If the pool drops its single connection, the next query opens a new
    // connection with a fresh empty database and the sessions table is lost.
    // min_connections(1) keeps the connection alive for the pool's lifetime.
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    store
        .create("persist-test", &[Message::user("hello")])
        .await
        .expect("create");

    let sessions = store.list().await.expect("list");
    assert_eq!(sessions.len(), 1, "session should persist across queries");
    assert_eq!(sessions[0].id, "persist-test");
}
