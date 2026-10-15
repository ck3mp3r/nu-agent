use super::*;

// ---------------------------------------------------------------------------
// Fix 1 tests: repair runs on cache-hit load
// ---------------------------------------------------------------------------

#[tokio::test]
async fn load_cache_hit_runs_repair() {
    let tmp = TempDir::new().unwrap();
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));

    mem.append(
        &ConversationId::from("conv-1"),
        vec![
            Message::user("first"),
            Message::user("second"),
            Message::assistant("reply"),
        ],
    )
    .await
    .unwrap();

    let loaded = mem.load(&ConversationId::from("conv-1")).await.unwrap();

    assert_eq!(loaded.len(), 2, "consecutive users should be merged");
    assert!(
        matches!(&loaded[0], Message::User { .. }),
        "first message should be user"
    );
    assert!(
        matches!(&loaded[1], Message::Assistant(_)),
        "second message should be assistant"
    );
}

/// `load` returns the raw stored messages (no marker summary prepended) and
/// serves subsequent reads from the cache.
#[tokio::test]
async fn load_returns_raw_messages_and_is_cached() {
    // -- Setup & Fixtures
    let tmp = TempDir::new().unwrap();
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));
    let store = FsSessionStore::new(tmp.path().to_path_buf());

    // -- Exec & Check: an empty session loads as empty
    let messages = mem.load(&ConversationId::from("conv-empty")).await.unwrap();
    assert!(messages.is_empty());

    // -- Exec & Check: messages written directly to the store are returned
    let msgs = vec![Message::user("hello"), Message::assistant("hi")];
    store.create("conv-1", &msgs).await.unwrap();
    let loaded = mem.load(&ConversationId::from("conv-1")).await.unwrap();
    assert_msgs_eq(&loaded, &msgs);

    // -- Exec & Check: a marker summary is not prepended as a system message
    let old = vec![Message::user("old1"), Message::assistant("old2")];
    store.create("conv-2", &old).await.unwrap();
    let marker = CompactionMarker::new("Summary of old stuff".to_string(), Utc::now());
    store
        .append("conv-2", &[StoreEntry::Marker(marker)])
        .await
        .unwrap();
    let recent = [Message::user("recent1"), Message::assistant("recent2")];
    let entries: Vec<StoreEntry> = recent.iter().cloned().map(StoreEntry::Message).collect();
    store.append("conv-2", &entries).await.unwrap();

    let loaded = mem.load(&ConversationId::from("conv-2")).await.unwrap();
    assert!(
        !loaded.iter().any(|m| matches!(m, Message::System { .. })),
        "load() must not prepend a marker summary as a system message, got: {loaded:?}"
    );
    assert_eq!(loaded.len(), 4, "all raw messages are preserved");
    assert_msg_eq(&loaded[0], &old[0]);
    assert_msg_eq(&loaded[1], &old[1]);
    assert_msg_eq(&loaded[2], &recent[0]);
    assert_msg_eq(&loaded[3], &recent[1]);

    // -- Exec & Check: a second load hits the cache instead of re-reading JSONL
    let first = mem.load(&ConversationId::from("conv-1")).await.unwrap();
    assert_eq!(first.len(), 2);
    let extra_entries: Vec<StoreEntry> = vec![StoreEntry::Message(Message::user("extra"))];
    store.append("conv-1", &extra_entries).await.unwrap();
    let second = mem.load(&ConversationId::from("conv-1")).await.unwrap();
    assert_eq!(
        second.len(),
        2,
        "second load must hit cache, not re-read JSONL"
    );
}
