use super::*;

// ================================================================
// FsSessionStore trait implementation tests
// ================================================================

/// `list` reports the sessions on disk and `delete` removes one.
#[tokio::test]
async fn fs_store_list_and_delete() {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    // -- Exec & Check: an empty directory lists no sessions
    let sessions = store.list().await.unwrap();
    assert_eq!(sessions.len(), 0, "empty directory lists no sessions");

    // -- Exec & Check: created sessions are listed
    store
        .create("abc1234-foo", &[Message::user("a")])
        .await
        .unwrap();
    store
        .create("abc1234-bar", &[Message::user("b")])
        .await
        .unwrap();
    store
        .create("def5678-baz", &[Message::user("c")])
        .await
        .unwrap();
    assert_eq!(store.list().await.unwrap().len(), 3);

    // -- Exec & Check: delete removes the session file
    store
        .create("delete-me", &[Message::user("bye")])
        .await
        .unwrap();
    assert!(store.load("delete-me").await.unwrap().is_some());
    store.delete("delete-me").await.unwrap();
    assert!(store.load("delete-me").await.unwrap().is_none());
}

#[tokio::test]
async fn fs_store_replace_entries_preserves_metadata() -> TestResult<()> {
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    // Create initial session
    store
        .create("replace-test", &[Message::user("original")])
        .await
        .unwrap();

    // Replace entries
    let new_entries = vec![
        StoreEntry::Message(Message::user("replaced")),
        StoreEntry::Message(Message::assistant("response")),
    ];
    store
        .replace_entries("replace-test", &new_entries)
        .await
        .unwrap();

    let (_metadata, entries) = store
        .load("replace-test")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), 2);
    let StoreEntry::Message(m) = &entries[0] else {
        return Err("expected Message entry".into());
    };
    assert!(matches!(m, Message::User { .. }));
    Ok(())
}
