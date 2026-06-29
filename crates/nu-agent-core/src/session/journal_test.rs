use super::journal::JournalConversationMemory;
use super::store::{CompactionMarker, ConversationStore, JsonlConversationStore};
use crate::types::Message;
use rig::memory::ConversationMemory;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fix 1 tests: repair runs on cache-hit load
// ---------------------------------------------------------------------------

#[tokio::test]
async fn load_cache_hit_runs_repair() {
    // Prime the cache with two consecutive user messages via append().
    // repair_messages() would merge them — but append() populates the cache
    // directly without repairing, so the bad state is in the cache.
    // The second load() must be a cache-hit AND must return repaired messages.
    let tmp = TempDir::new().unwrap();
    let mem = JournalConversationMemory::new(tmp.path().to_path_buf());

    // Append two consecutive user messages and one assistant — this goes
    // directly into the in-memory cache (no repair on the write path).
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

    // First load is a cache hit (cache was populated by append above).
    // It must run repair and merge the consecutive user messages.
    let loaded = mem.load("conv-1").await.unwrap();

    // After merging consecutive users: [User("first" + "second"), Assistant("reply")]
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

// ---------------------------------------------------------------------------
// Fix 2 tests: append() preserves last known token count
// ---------------------------------------------------------------------------

#[tokio::test]
async fn append_preserves_last_total_tokens_when_none() {
    // Scenario: a conversation already has a known token count from a
    // previous successful turn (written via append_messages_to_store_only).
    // A subsequent append() call (e.g., from the error-fallback path) must
    // NOT clobber that count with null.
    let tmp = TempDir::new().unwrap();
    let mem = JournalConversationMemory::new(tmp.path().to_path_buf());

    // Write an initial message with a known token count to the JSONL store.
    let initial = vec![Message::user("hello"), Message::assistant("hi")];
    mem.append_messages_to_store_only("conv-1", &initial, Some(5000))
        .unwrap();

    // Populate the in-memory cache.
    let _ = mem.load("conv-1").await.unwrap();

    // Now append via the trait method (no token count available — simulates
    // the error-fallback path in executor.rs).
    let fallback = vec![
        Message::user("failed prompt"),
        Message::assistant("[Turn failed: some error]"),
    ];
    mem.append("conv-1", fallback).await.unwrap();

    // Read raw JSONL and verify the last entry preserves the token count.
    let raw = std::fs::read_to_string(tmp.path().join("conv-1.jsonl")).unwrap();
    let last_data_line = raw.lines().rfind(|l| !l.trim().is_empty()).unwrap();
    let value: serde_json::Value = serde_json::from_str(last_data_line).unwrap();
    assert_eq!(
        value["last_total_tokens"],
        serde_json::json!(5000),
        "append() must preserve the last known token count (5000) — \
         got: {}",
        value["last_total_tokens"]
    );
}

/// Compare two messages via their serialized JSON form.
///
/// rig uses `#[serde(flatten)]` on `Text::additional_params`.
/// A round-trip through serde turns `None` into `Some(Object {})`,
/// which breaks `PartialEq` even though the two forms are semantically
/// identical. Serializing first normalizes both sides.
fn assert_msg_eq(left: &Message, right: &Message) {
    assert_eq!(
        serde_json::to_value(left).unwrap(),
        serde_json::to_value(right).unwrap(),
    );
}

fn assert_msgs_eq(left: &[Message], right: &[Message]) {
    assert_eq!(left.len(), right.len(), "message count mismatch");
    for (i, (l, r)) in left.iter().zip(right.iter()).enumerate() {
        assert_eq!(
            serde_json::to_value(l).unwrap(),
            serde_json::to_value(r).unwrap(),
            "message {i} mismatch",
        );
    }
}

#[tokio::test]
async fn load_empty_returns_empty() {
    let tmp = TempDir::new().unwrap();
    let mem = JournalConversationMemory::new(tmp.path().to_path_buf());

    let messages = mem.load("conv-1").await.unwrap();
    assert!(messages.is_empty());
}

#[tokio::test]
async fn load_returns_stored_messages() {
    let tmp = TempDir::new().unwrap();
    let mem = JournalConversationMemory::new(tmp.path().to_path_buf());

    // Write messages to JSONL manually via the underlying store
    let store = JsonlConversationStore::new(tmp.path().to_path_buf());
    let msgs = vec![Message::user("hello"), Message::assistant("hi")];
    store.append("conv-1", &msgs, None).unwrap();

    let loaded = mem.load("conv-1").await.unwrap();
    assert_msgs_eq(&loaded, &msgs);
}

#[tokio::test]
async fn load_uses_extract_llm_context() {
    let tmp = TempDir::new().unwrap();
    let mem = JournalConversationMemory::new(tmp.path().to_path_buf());

    // Write messages + marker + recent messages to JSONL
    let store = JsonlConversationStore::new(tmp.path().to_path_buf());
    let old = vec![
        Message::user("old1"),
        Message::assistant("old2"),
        Message::user("old3"),
    ];
    store.append("conv-1", &old, None).unwrap();

    let marker = CompactionMarker::new("Summary of old stuff".to_string(), 2, 3, "sliding_summary");
    store.append_marker("conv-1", &marker, None).unwrap();

    let recent = vec![Message::user("recent1"), Message::assistant("recent2")];
    store.append("conv-1", &recent, None).unwrap();

    let loaded = mem.load("conv-1").await.unwrap();

    // extract_llm_context: [System(summary)] + recent
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
    let mem = JournalConversationMemory::new(tmp.path().to_path_buf());

    // Write messages directly to the backing store (not via mem.append — that
    // would populate the cache before we exercise the cold-start JSONL path).
    let store = JsonlConversationStore::new(tmp.path().to_path_buf());
    let msgs = vec![Message::user("hello"), Message::assistant("hi")];
    store.append("conv-1", &msgs, None).unwrap();

    // First load: cache miss — reads from JSONL and populates cache
    let first = mem.load("conv-1").await.unwrap();
    assert_eq!(first.len(), 2);
    let count_after_first = mem.compaction_count();

    // Mutate the JSONL file — add more messages (bypasses cache)
    store
        .append("conv-1", &[Message::user("extra")], None)
        .unwrap();

    // Second load: cache hit — JSONL mutation must NOT be visible
    let second = mem.load("conv-1").await.unwrap();
    assert_eq!(
        second.len(),
        2,
        "second load must hit cache, not re-read JSONL"
    );

    // compaction_count must be unchanged — only updates on cache-miss
    assert_eq!(
        mem.compaction_count(),
        count_after_first,
        "compaction_count must not change on a cache-hit load"
    );
}

#[tokio::test]
async fn append_writes_to_memory_and_store() {
    let tmp = TempDir::new().unwrap();
    let mem = JournalConversationMemory::new(tmp.path().to_path_buf());

    let msgs = vec![Message::user("hello"), Message::assistant("hi")];
    mem.append("conv-1", msgs.clone()).await.unwrap();

    // Check in-memory cache via subsequent load (which reads from cache)
    let cached = mem.load("conv-1").await.unwrap();
    assert_msgs_eq(&cached, &msgs);

    // Check JSONL store — read raw via the backing store
    let store = JsonlConversationStore::new(tmp.path().to_path_buf());
    let (entries, _) = store.load_all("conv-1").unwrap();
    assert_eq!(entries.len(), msgs.len());
}

#[tokio::test]
async fn clear_resets_cache_not_store() {
    let tmp = TempDir::new().unwrap();
    let mem = JournalConversationMemory::new(tmp.path().to_path_buf());

    let msgs = vec![Message::user("hello"), Message::assistant("hi")];
    mem.append("conv-1", msgs.clone()).await.unwrap();

    // Clear — only removes from in-memory cache
    mem.clear("conv-1").await.unwrap();

    // JSONL should still have messages
    let store = JsonlConversationStore::new(tmp.path().to_path_buf());
    let (entries, _) = store.load_all("conv-1").unwrap();
    assert_eq!(entries.len(), msgs.len(), "JSONL should not be cleared");

    // Subsequent load should re-read from JSONL
    let reloaded = mem.load("conv-1").await.unwrap();
    assert_msgs_eq(&reloaded, &msgs);
}

#[tokio::test]
async fn append_after_clear_no_duplicate() {
    let tmp = TempDir::new().unwrap();
    let mem = JournalConversationMemory::new(tmp.path().to_path_buf());

    let msgs = vec![Message::user("hello"), Message::assistant("hi")];

    // First append
    mem.append("conv-1", msgs.clone()).await.unwrap();

    // Clear in-memory cache
    mem.clear("conv-1").await.unwrap();

    // Append the same messages again
    mem.append("conv-1", msgs.clone()).await.unwrap();

    // JSONL should have each message ONCE (because we appended twice to JSONL)
    // Actually the second append goes to JSONL again — so JSONL has 4 entries total.
    // But the behavior is: clear resets in-memory only. JSONL is append-only.
    // The important thing: no unexpected duplication within a single append call.
    let store = JsonlConversationStore::new(tmp.path().to_path_buf());
    let (entries, _) = store.load_all("conv-1").unwrap();
    // Each append writes to JSONL independently, so 2 appends = 4 entries in JSONL
    assert_eq!(
        entries.len(),
        msgs.len() * 2,
        "each append should write exactly the messages once"
    );
}

#[tokio::test]
async fn reset_context_replaces_cache_only() {
    let tmp = TempDir::new().unwrap();
    let mem = JournalConversationMemory::new(tmp.path().to_path_buf());

    // Write to store
    let store = JsonlConversationStore::new(tmp.path().to_path_buf());
    let original = vec![Message::user("original")];
    store.append("conv-1", &original, None).unwrap();

    // Populate cache via load
    let _ = mem.load("conv-1").await.unwrap();

    // Replace cache with new messages
    let new_msgs = vec![Message::user("replaced"), Message::assistant("answer")];
    mem.reset_context("conv-1", new_msgs.clone());

    // Cache should reflect new messages
    let cached = mem.load("conv-1").await.unwrap();
    assert_msgs_eq(&cached, &new_msgs);

    // JSONL should be unchanged
    let (entries, _) = store.load_all("conv-1").unwrap();
    assert_eq!(
        entries.len(),
        1,
        "JSONL should still have only the original message"
    );
}

#[tokio::test]
async fn compaction_count_populated_after_load() {
    let tmp = TempDir::new().unwrap();
    let mem = JournalConversationMemory::new(tmp.path().to_path_buf());

    // Write two compaction markers to the JSONL
    let store = JsonlConversationStore::new(tmp.path().to_path_buf());
    store
        .append("conv-1", &[Message::user("m1")], None)
        .unwrap();

    let marker1 = CompactionMarker::new("Summary1".to_string(), 1, 1, "sliding_summary");
    store.append_marker("conv-1", &marker1, None).unwrap();
    store
        .append("conv-1", &[Message::user("m2")], None)
        .unwrap();

    let marker2 = CompactionMarker::new("Summary2".to_string(), 1, 2, "sliding_summary");
    store.append_marker("conv-1", &marker2, None).unwrap();
    store
        .append("conv-1", &[Message::user("m3")], None)
        .unwrap();

    // Load to populate compaction_count
    let _ = mem.load("conv-1").await.unwrap();

    assert_eq!(mem.compaction_count(), 2);
}

#[tokio::test]
async fn append_writes_null_tokens_to_store_when_no_prior_count() {
    // When there is no prior token count in the store, append() must write
    // null (not invent a value). Token counts are only preserved when a
    // previous entry already recorded a non-null value.
    let tmp = TempDir::new().unwrap();
    let mem = JournalConversationMemory::new(tmp.path().to_path_buf());

    mem.append("conv-1", vec![Message::user("hi")])
        .await
        .unwrap();

    // Read raw JSONL and verify last_total_tokens is null (no prior count to preserve)
    let raw = std::fs::read_to_string(tmp.path().join("conv-1.jsonl")).unwrap();
    let last_data_line = raw.lines().last().unwrap();
    let value: serde_json::Value = serde_json::from_str(last_data_line).unwrap();
    assert!(
        value["last_total_tokens"].is_null(),
        "append() must write null when there is no prior token count to preserve; \
         got: {}",
        value["last_total_tokens"]
    );
}

#[tokio::test]
async fn append_marker_writes_to_store_only() {
    let tmp = TempDir::new().unwrap();
    let mem = JournalConversationMemory::new(tmp.path().to_path_buf());

    // First load to initialize cache (empty)
    let _ = mem.load("conv-1").await.unwrap();

    let marker = CompactionMarker::new("Summary".to_string(), 2, 5, "sliding_summary");
    mem.append_marker("conv-1", &marker, None).unwrap();

    // JSONL should have the marker
    let store = JsonlConversationStore::new(tmp.path().to_path_buf());
    let (entries, _) = store.load_all("conv-1").unwrap();
    assert_eq!(entries.len(), 1);
    assert!(matches!(&entries[0], super::store::StoreEntry::Marker(_)));

    // Cache should NOT contain the marker (clear + reload would re-read it)
    // After append_marker, the cache is unchanged (it was empty)
    let cached = mem.load("conv-1").await.unwrap();
    assert!(
        cached.is_empty(),
        "cache should not be updated by append_marker"
    );
}

#[tokio::test]
async fn append_messages_to_store_only_no_cache_update() {
    let tmp = TempDir::new().unwrap();
    let mem = JournalConversationMemory::new(tmp.path().to_path_buf());

    // Populate cache with initial messages via load
    let store = JsonlConversationStore::new(tmp.path().to_path_buf());
    let initial = vec![Message::user("initial"), Message::assistant("reply")];
    store.append("conv-1", &initial, None).unwrap();
    let _ = mem.load("conv-1").await.unwrap(); // fills cache

    // Append to store only (no cache update)
    let extra = vec![Message::user("store-only")];
    mem.append_messages_to_store_only("conv-1", &extra, None)
        .unwrap();

    // JSONL has both
    let (entries, _) = store.load_all("conv-1").unwrap();
    assert_eq!(entries.len(), 3);

    // Cache should still have only the initial messages (no re-read happened)
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
    let memory = crate::session::JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let conversation_id = "integ-dedup-test";

    // Simulate turn 1 success: append [user, assistant]
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

    // Verify the store has exactly 2 entries (no duplication at the storage layer)
    let (entries_after_turn1, _) = store.load_all(conversation_id).unwrap();
    assert_eq!(
        entries_after_turn1.len(),
        2,
        "after turn1: JSONL store must have exactly 2 entries"
    );

    // Simulate turn 2 error: the executor appends the delta [user("turn2"),
    // assistant("[Turn failed:]")] — a complete pair so the session stays valid.
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

    // Verify the store has exactly 4 entries (strictly additive)
    let (entries_after_turn2, _) = store.load_all(conversation_id).unwrap();
    assert_eq!(
        entries_after_turn2.len(),
        4,
        "after turn2 delta: JSONL store must have exactly 4 entries (bug would double to >4)"
    );

    // load() must also return the correct count after complete pairs
    let after_turn2 = memory
        .load(conversation_id)
        .await
        .expect("load should succeed");
    assert_eq!(
        after_turn2.len(),
        4,
        "after turn2 delta: load() expected 4 messages"
    );

    // Simulate turn 3 error: append another delta pair
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

    // Verify the store has exactly 6 entries (strictly additive)
    let (entries_after_turn3, _) = store.load_all(conversation_id).unwrap();
    assert_eq!(
        entries_after_turn3.len(),
        6,
        "after turn3 delta: JSONL store must have exactly 6 entries (bug would double)"
    );

    // load() must return the correct count
    let after_turn3 = memory
        .load(conversation_id)
        .await
        .expect("load should succeed");
    assert_eq!(
        after_turn3.len(),
        6,
        "after turn3 delta: load() expected 6 messages"
    );

    // Verify no duplicate message content in the final view
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
