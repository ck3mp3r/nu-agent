use super::*;

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
