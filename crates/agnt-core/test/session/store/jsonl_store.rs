use super::*;

/// `create` + `load` round-trips messages, `append` adds to an existing
/// session, and `append` creates the session when it is missing.
#[tokio::test]
async fn jsonl_store_round_trip_and_append() -> TestResult<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    // -- Exec & Check: create then load round-trips the messages
    let messages = vec![
        Message::user("Hello"),
        Message::assistant("Hi there!"),
        Message::user("How are you?"),
    ];
    store.create("test-session", &messages).await.unwrap();
    let (_metadata, entries) = store
        .load("test-session")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), messages.len());
    for (entry, msg) in entries.iter().zip(messages.iter()) {
        let StoreEntry::Message(e) = entry else {
            return Err("expected Message entry".into());
        };
        assert_msg_eq(e, msg);
    }

    // -- Exec & Check: append adds messages to an existing session
    let initial_messages = vec![
        Message::user("First message"),
        Message::assistant("First response"),
    ];
    store
        .create("append-session", &initial_messages)
        .await
        .unwrap();
    let additional_messages = [
        Message::user("Second message"),
        Message::assistant("Second response"),
    ];
    let entries: Vec<StoreEntry> = additional_messages
        .iter()
        .cloned()
        .map(StoreEntry::Message)
        .collect();
    store.append("append-session", &entries).await.unwrap();

    let (_metadata, loaded) = store
        .load("append-session")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(loaded.len(), 4);
    for (entry, expected) in loaded
        .iter()
        .zip(initial_messages.iter().chain(additional_messages.iter()))
    {
        let StoreEntry::Message(m) = entry else {
            return Err("expected Message entry".into());
        };
        assert_msg_eq(m, expected);
    }

    // -- Exec & Check: append creates the session when it is missing
    let new_entries = vec![StoreEntry::Message(Message::user(
        "First message in new session",
    ))];
    store.append("new-session", &new_entries).await.unwrap();
    let (_metadata, loaded) = store
        .load("new-session")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(loaded.len(), 1);
    let StoreEntry::Message(m) = &loaded[0] else {
        return Err("expected Message entry".into());
    };
    assert_msg_eq(m, &Message::user("First message in new session"));
    Ok(())
}

/// Loading a missing session returns `None`; `delete` removes an existing one.
#[tokio::test]
async fn jsonl_store_load_missing_and_delete() {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    // -- Exec & Check: a missing session loads as None, not an error
    assert!(store.load("non-existent-session").await.unwrap().is_none());

    // -- Exec & Check: delete removes the session
    let messages = vec![Message::user("Hello"), Message::assistant("Hi")];
    store.create("test-session", &messages).await.unwrap();
    store.delete("test-session").await.unwrap();
    assert!(store.load("test-session").await.unwrap().is_none());
}

#[tokio::test]
async fn jsonl_store_handles_corrupt_lines() -> TestResult<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    let messages = vec![
        Message::user("Valid message 1"),
        Message::assistant("Valid response 1"),
    ];
    store.create("test-session", &messages).await.unwrap();

    // Manually append a corrupt line to the file
    let session_path = temp_dir.path().join("test-session.jsonl");
    std::fs::write(
        &session_path,
        std::fs::read_to_string(&session_path).unwrap()
            + "\nthis is not valid json\n"
            + &serde_json::to_string(&Message::user("Valid message 2")).unwrap()
            + "\n",
    )
    .unwrap();

    // -- Exec
    let (_metadata, entries) = store
        .load("test-session")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;

    // -- Check: the corrupt line is skipped, valid lines survive
    assert_eq!(entries.len(), 3); // 2 original + 1 after corrupt line
    let expected = [
        messages[0].clone(),
        messages[1].clone(),
        Message::user("Valid message 2"),
    ];
    for (entry, expected) in entries.iter().zip(expected.iter()) {
        let StoreEntry::Message(m) = entry else {
            return Err("expected Message entry".into());
        };
        assert_msg_eq(m, expected);
    }
    Ok(())
}

// --- CompactionMarker and StoreEntry tests ---

/// `CompactionMarker` round-trips through serde and tolerates old JSON that
/// omits `created_at`.
#[test]
fn compaction_marker_serde() -> TestResult<()> {
    // -- Setup & Fixtures
    let marker = CompactionMarker::new("Summary of old messages".to_string(), Utc::now());
    let old_json = r#"{
        "type": "compaction_marker",
        "summary": "old summary"
    }"#;

    // -- Exec
    let json = serde_json::to_string(&marker).unwrap();
    let deserialized: CompactionMarker = serde_json::from_str(&json).unwrap();
    let old: CompactionMarker = serde_json::from_str(old_json)?;

    // -- Check
    assert_eq!(deserialized.entry_type, "compaction_marker");
    assert_eq!(deserialized.summary, "Summary of old messages");
    assert_eq!(deserialized.created_at, marker.created_at);
    assert_eq!(old.summary, "old summary");
    assert_eq!(old.created_at, chrono::DateTime::<Utc>::UNIX_EPOCH);
    Ok(())
}

/// A marker appended to the store is written to the raw JSONL with its
/// `type` field and is returned by `load`.
#[tokio::test]
async fn append_marker_writes_to_store() -> TestResult<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());
    store
        .create("test-session", &[Message::user("Hello")])
        .await
        .unwrap();

    // -- Exec
    let marker = CompactionMarker::new("Summary".to_string(), Utc::now());
    store
        .append("test-session", &[StoreEntry::Marker(marker)])
        .await
        .unwrap();

    // -- Check: raw JSONL carries the type tag
    let content = std::fs::read_to_string(temp_dir.path().join("test-session.jsonl")).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    let value: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(
        value["type"].as_str(),
        Some("compaction_marker"),
        "raw JSONL must contain \"type\":\"compaction_marker\", got: {value}"
    );

    // -- Check: load returns the marker as the second entry
    let (_metadata, entries) = store
        .load("test-session")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), 2);
    let StoreEntry::Marker(m) = &entries[1] else {
        return Err("expected marker as second entry".into());
    };
    assert_eq!(m.summary, "Summary");
    Ok(())
}

/// `load` returns messages and markers in their original order.
#[tokio::test]
async fn load_returns_entries_in_order() -> TestResult<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());
    let initial = vec![Message::user("m1"), Message::assistant("r1")];
    store.create("test-session", &initial).await.unwrap();

    let marker = CompactionMarker::new("S1".to_string(), Utc::now());
    store
        .append("test-session", &[StoreEntry::Marker(marker)])
        .await
        .unwrap();

    let follow_up: Vec<StoreEntry> = vec![
        StoreEntry::Message(Message::user("m2")),
        StoreEntry::Message(Message::assistant("r2")),
    ];
    store.append("test-session", &follow_up).await.unwrap();

    // -- Exec
    let (_metadata, entries) = store
        .load("test-session")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;

    // -- Check
    assert_eq!(entries.len(), 5); // 2 msgs + 1 marker + 2 msgs
    assert!(matches!(&entries[0], StoreEntry::Message(_)));
    assert!(matches!(&entries[1], StoreEntry::Message(_)));
    assert!(matches!(&entries[2], StoreEntry::Marker(_)));
    assert!(matches!(&entries[3], StoreEntry::Message(_)));
    assert!(matches!(&entries[4], StoreEntry::Message(_)));
    Ok(())
}
