use super::*;

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
