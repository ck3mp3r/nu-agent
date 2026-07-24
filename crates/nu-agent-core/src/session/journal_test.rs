use super::journal::CachedMemory;
use super::store::{CompactionMarker, FsSessionStore, SessionStore as _, StoreEntry};
use super::store_test::{assert_msg_eq, assert_msgs_eq};
use crate::types::Message;
use rig::memory::ConversationMemory;
use std::sync::Arc;
use tempfile::TempDir;

type TestMemory = CachedMemory<FsSessionStore>;

// ---------------------------------------------------------------------------
// Fix 1 tests: repair runs on cache-hit load
// ---------------------------------------------------------------------------

#[tokio::test]
async fn load_cache_hit_runs_repair() {
    let tmp = TempDir::new().unwrap();
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));

    mem.append(
        "conv-1",
        vec![
            Message::user("first"),
            Message::user("second"),
            Message::assistant("reply"),
        ],
    )
    .await
    .unwrap();

    let loaded = mem.load("conv-1").await.unwrap();

    assert_eq!(loaded.len(), 2, "consecutive users should be merged");
    assert!(
        matches!(&loaded[0], Message::User { .. }),
        "first message should be user"
    );
    assert!(
        matches!(&loaded[1], Message::Assistant { .. }),
        "second message should be assistant"
    );
}

#[tokio::test]
async fn load_empty_returns_empty() {
    let tmp = TempDir::new().unwrap();
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));

    let messages = mem.load("conv-1").await.unwrap();
    assert!(messages.is_empty());
}

#[tokio::test]
async fn load_returns_stored_messages() {
    let tmp = TempDir::new().unwrap();
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));

    // Write messages to store directly
    let store = FsSessionStore::new(tmp.path().to_path_buf());
    let msgs = vec![Message::user("hello"), Message::assistant("hi")];
    store.create("conv-1", &msgs).await.unwrap();

    let loaded = mem.load("conv-1").await.unwrap();
    assert_msgs_eq(&loaded, &msgs);
}

#[tokio::test]
async fn load_uses_extract_llm_context() {
    let tmp = TempDir::new().unwrap();
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));

    let store = FsSessionStore::new(tmp.path().to_path_buf());
    let old = vec![
        Message::user("old1"),
        Message::assistant("old2"),
        Message::user("old3"),
    ];
    store.create("conv-1", &old).await.unwrap();

    let marker = CompactionMarker::new("Summary of old stuff".to_string(), 2, 3, "sliding_summary");
    store
        .append("conv-1", &[StoreEntry::Marker(marker)])
        .await
        .unwrap();

    let recent = [Message::user("recent1"), Message::assistant("recent2")];
    let entries: Vec<StoreEntry> = recent.iter().cloned().map(StoreEntry::Message).collect();
    store.append("conv-1", &entries).await.unwrap();

    let loaded = mem.load("conv-1").await.unwrap();

    assert_eq!(loaded.len(), 3); // 1 system + 2 recent
    assert!(
        matches!(&loaded[0], Message::System { content } if content == "Summary of old stuff"),
        "First message should be system summary, got: {:?}",
        loaded[0]
    );
    assert_msg_eq(&loaded[1], &recent[0]);
    assert_msg_eq(&loaded[2], &recent[1]);
}

#[tokio::test]
async fn load_is_cached() {
    let tmp = TempDir::new().unwrap();
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));

    let store = FsSessionStore::new(tmp.path().to_path_buf());
    let msgs = vec![Message::user("hello"), Message::assistant("hi")];
    store.create("conv-1", &msgs).await.unwrap();

    // First load: cache miss
    let first = mem.load("conv-1").await.unwrap();
    assert_eq!(first.len(), 2);

    // Mutate the JSONL file — add more messages (bypasses cache)
    let extra_entries: Vec<StoreEntry> = vec![StoreEntry::Message(Message::user("extra"))];
    store.append("conv-1", &extra_entries).await.unwrap();

    // Second load: cache hit
    let second = mem.load("conv-1").await.unwrap();
    assert_eq!(
        second.len(),
        2,
        "second load must hit cache, not re-read JSONL"
    );
}

#[tokio::test]
async fn append_writes_to_memory_and_store() {
    let tmp = TempDir::new().unwrap();
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));

    let msgs = vec![Message::user("hello"), Message::assistant("hi")];
    mem.append("conv-1", msgs.clone()).await.unwrap();

    let cached = mem.load("conv-1").await.unwrap();
    assert_msgs_eq(&cached, &msgs);

    // Check store via load_all
    let entries = mem.load_all("conv-1").await.unwrap();
    assert_eq!(entries.len(), msgs.len());
}

#[tokio::test]
async fn clear_resets_cache_not_store() {
    let tmp = TempDir::new().unwrap();
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));

    let msgs = vec![Message::user("hello"), Message::assistant("hi")];
    mem.append("conv-1", msgs.clone()).await.unwrap();

    mem.clear("conv-1").await.unwrap();

    // Store still has messages
    let entries = mem.load_all("conv-1").await.unwrap();
    assert_eq!(entries.len(), msgs.len(), "Store should not be cleared");

    // Subsequent load should re-read from store
    let reloaded = mem.load("conv-1").await.unwrap();
    assert_msgs_eq(&reloaded, &msgs);
}

#[tokio::test]
async fn append_after_clear_no_duplicate() {
    let tmp = TempDir::new().unwrap();
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));

    let msgs = vec![Message::user("hello"), Message::assistant("hi")];

    mem.append("conv-1", msgs.clone()).await.unwrap();
    mem.clear("conv-1").await.unwrap();
    mem.append("conv-1", msgs.clone()).await.unwrap();

    let entries = mem.load_all("conv-1").await.unwrap();
    assert_eq!(
        entries.len(),
        msgs.len() * 2,
        "each append should write exactly the messages once"
    );
}

#[tokio::test]
async fn reset_context_replaces_cache_only() {
    let tmp = TempDir::new().unwrap();
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));

    let store = FsSessionStore::new(tmp.path().to_path_buf());
    let original = vec![Message::user("original")];
    store.create("conv-1", &original).await.unwrap();

    let _ = mem.load("conv-1").await.unwrap();

    let new_msgs = vec![Message::user("replaced"), Message::assistant("answer")];
    mem.reset_context("conv-1", new_msgs.clone());

    let cached = mem.load("conv-1").await.unwrap();
    assert_msgs_eq(&cached, &new_msgs);

    // Store unchanged
    let entries = mem.load_all("conv-1").await.unwrap();
    assert_eq!(
        entries.len(),
        1,
        "Store should still have only the original message"
    );
}

#[tokio::test]
async fn append_marker_writes_to_store_only() {
    let tmp = TempDir::new().unwrap();
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));

    let _ = mem.load("conv-1").await.unwrap();

    let marker = CompactionMarker::new("Summary".to_string(), 2, 5, "sliding_summary");
    mem.append_marker("conv-1", &marker).await.unwrap();

    let entries = mem.load_all("conv-1").await.unwrap();
    assert_eq!(entries.len(), 1);
    assert!(matches!(&entries[0], StoreEntry::Marker(_)));

    let cached = mem.load("conv-1").await.unwrap();
    assert!(
        cached.is_empty(),
        "cache should not be updated by append_marker"
    );
}

#[tokio::test]
async fn append_messages_to_store_only_no_cache_update() {
    let tmp = TempDir::new().unwrap();
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));

    let store = FsSessionStore::new(tmp.path().to_path_buf());
    let initial = vec![Message::user("initial"), Message::assistant("reply")];
    store.create("conv-1", &initial).await.unwrap();
    let _ = mem.load("conv-1").await.unwrap();

    let extra = vec![Message::user("store-only")];
    mem.append_messages_to_store_only("conv-1", &extra)
        .await
        .unwrap();

    let entries = mem.load_all("conv-1").await.unwrap();
    assert_eq!(entries.len(), 3);

    let cached = mem.load("conv-1").await.unwrap();
    assert_eq!(
        cached.len(),
        2,
        "cache should not be updated by append_messages_to_store_only"
    );
    assert_msg_eq(&cached[0], &initial[0]);
    assert_msg_eq(&cached[1], &initial[1]);
}

// ---------------------------------------------------------------------------
// Integration test: no duplication when appending deltas sequentially
// ---------------------------------------------------------------------------

#[tokio::test]
async fn no_duplication_when_appending_deltas_sequentially() {
    use rig::memory::ConversationMemory;

    let temp_dir = tempfile::tempdir().unwrap();
    let memory = TestMemory::new(Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf())));
    let conversation_id = "integ-dedup-test";

    memory
        .append(
            conversation_id,
            vec![
                crate::types::Message::user("turn1"),
                crate::types::Message::assistant("ok1"),
            ],
        )
        .await
        .expect("append turn1 should succeed");

    let entries_after_turn1 = memory.load_all(conversation_id).await.unwrap();
    assert_eq!(
        entries_after_turn1.len(),
        2,
        "after turn1: store must have exactly 2 entries"
    );

    memory
        .append(
            conversation_id,
            vec![
                crate::types::Message::user("turn2"),
                crate::types::Message::assistant("[Turn failed: network error]"),
            ],
        )
        .await
        .expect("append turn2 delta should succeed");

    let entries_after_turn2 = memory.load_all(conversation_id).await.unwrap();
    assert_eq!(
        entries_after_turn2.len(),
        4,
        "after turn2 delta: store must have exactly 4 entries"
    );

    let after_turn2 = memory
        .load(conversation_id)
        .await
        .expect("load should succeed");
    assert_eq!(
        after_turn2.len(),
        4,
        "after turn2 delta: load() expected 4 messages"
    );

    memory
        .append(
            conversation_id,
            vec![
                crate::types::Message::user("turn3"),
                crate::types::Message::assistant("[Turn failed: timeout]"),
            ],
        )
        .await
        .expect("append turn3 delta should succeed");

    let entries_after_turn3 = memory.load_all(conversation_id).await.unwrap();
    assert_eq!(
        entries_after_turn3.len(),
        6,
        "after turn3 delta: store must have exactly 6 entries"
    );

    let after_turn3 = memory
        .load(conversation_id)
        .await
        .expect("load should succeed");
    assert_eq!(
        after_turn3.len(),
        6,
        "after turn3 delta: load() expected 6 messages"
    );

    let texts: Vec<String> = after_turn3
        .iter()
        .filter_map(|msg| match msg {
            crate::types::Message::User { content } => content.iter().find_map(|c| {
                if let crate::types::UserContent::Text(t) = c {
                    Some(t.text.clone())
                } else {
                    None
                }
            }),
            crate::types::Message::Assistant { content, .. } => content.iter().find_map(|c| {
                if let crate::types::AssistantContent::Text(t) = c {
                    Some(t.text.clone())
                } else {
                    None
                }
            }),
            _ => None,
        })
        .collect();

    let unique_count = texts.iter().collect::<std::collections::HashSet<_>>().len();
    assert_eq!(
        unique_count,
        texts.len(),
        "no message content should appear twice; got: {:?}",
        texts
    );
}
