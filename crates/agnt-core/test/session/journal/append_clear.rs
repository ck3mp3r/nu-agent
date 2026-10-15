use super::*;

/// `append` writes to both the cache and the store; `clear` resets only the
/// cache; `reset_context` replaces only the cache; and a re-append after a
/// clear writes each message exactly once.
#[tokio::test]
async fn append_clear_and_reset_context_cache_store_contract() {
    // -- Setup & Fixtures
    let tmp = TempDir::new().unwrap();
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));
    let conv = ConversationId::from("conv-1");
    let msgs = vec![Message::user("hello"), Message::assistant("hi")];

    // -- Exec & Check: append writes to memory and store
    mem.append(&conv, msgs.clone()).await.unwrap();
    let cached = mem.load(&conv).await.unwrap();
    assert_msgs_eq(&cached, &msgs);
    assert_eq!(mem.load_all("conv-1").await.unwrap().len(), msgs.len());

    // -- Exec & Check: clear resets the cache but not the store
    mem.clear(&conv).await.unwrap();
    assert_eq!(
        mem.load_all("conv-1").await.unwrap().len(),
        msgs.len(),
        "store must not be cleared"
    );
    let reloaded = mem.load(&conv).await.unwrap();
    assert_msgs_eq(&reloaded, &msgs);

    // -- Exec & Check: append after clear writes each message exactly once
    mem.clear(&conv).await.unwrap();
    mem.append(&conv, msgs.clone()).await.unwrap();
    assert_eq!(
        mem.load_all("conv-1").await.unwrap().len(),
        msgs.len() * 2,
        "each append writes exactly the messages once"
    );

    // -- Exec & Check: reset_context replaces the cache only
    let store = FsSessionStore::new(tmp.path().to_path_buf());
    let original = vec![Message::user("original")];
    store.create("conv-2", &original).await.unwrap();
    let conv2 = ConversationId::from("conv-2");
    let _ = mem.load(&conv2).await.unwrap();

    let new_msgs = vec![Message::user("replaced"), Message::assistant("answer")];
    mem.reset_context("conv-2", new_msgs.clone());

    let cached = mem.load(&conv2).await.unwrap();
    assert_msgs_eq(&cached, &new_msgs);
    assert_eq!(
        mem.load_all("conv-2").await.unwrap().len(),
        1,
        "store keeps only the original message"
    );
}

/// `append_marker` and `append_messages_to_store_only` write to the store
/// without touching the cache.
#[tokio::test]
async fn store_only_appends_leave_cache_untouched() {
    // -- Setup & Fixtures
    let tmp = TempDir::new().unwrap();
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));
    let conv = ConversationId::from("conv-1");
    let _ = mem.load(&conv).await.unwrap();

    // -- Exec & Check: append_marker writes a marker to the store only
    let marker = CompactionMarker::new("Summary".to_string(), Utc::now());
    mem.append_marker("conv-1", &marker).await.unwrap();

    let entries = mem.load_all("conv-1").await.unwrap();
    assert_eq!(entries.len(), 1);
    assert!(matches!(&entries[0], StoreEntry::Marker(_)));
    assert!(
        mem.load(&conv).await.unwrap().is_empty(),
        "cache must not be updated by append_marker"
    );

    // -- Exec & Check: append_messages_to_store_only writes messages to the store only
    let store = FsSessionStore::new(tmp.path().to_path_buf());
    let initial = vec![Message::user("initial"), Message::assistant("reply")];
    store.create("conv-2", &initial).await.unwrap();
    let conv2 = ConversationId::from("conv-2");
    let _ = mem.load(&conv2).await.unwrap();

    let extra = vec![Message::user("store-only")];
    mem.append_messages_to_store_only("conv-2", &extra)
        .await
        .unwrap();

    assert_eq!(mem.load_all("conv-2").await.unwrap().len(), 3);
    let cached = mem.load(&conv2).await.unwrap();
    assert_eq!(
        cached.len(),
        2,
        "cache must not be updated by append_messages_to_store_only"
    );
    assert_msg_eq(&cached[0], &initial[0]);
    assert_msg_eq(&cached[1], &initial[1]);
}
