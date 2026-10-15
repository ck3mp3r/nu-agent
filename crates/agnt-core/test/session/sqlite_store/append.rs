use super::*;

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
