use super::journal::CachedMemory;
use super::store::{CompactionMarker, FsSessionStore, SessionStore as _, StoreEntry};
use super::store_test::{assert_msg_eq, assert_msgs_eq};
use crate::types::{Message, Text, ToolCallId, ToolResult, ToolResultContent, UserContent};
use chrono::Utc;
use rig::memory::ConversationMemory;
use std::sync::Arc;
use tempfile::TempDir;

type TestMemory = CachedMemory<FsSessionStore>;
type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

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
async fn load_returns_raw_messages_without_marker_summary() {
    let tmp = TempDir::new().unwrap();
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));

    let store = FsSessionStore::new(tmp.path().to_path_buf());
    let old = vec![Message::user("old1"), Message::assistant("old2")];
    store.create("conv-1", &old).await.unwrap();

    let marker = CompactionMarker::new("Summary of old stuff".to_string(), Utc::now());
    store
        .append("conv-1", &[StoreEntry::Marker(marker)])
        .await
        .unwrap();

    let recent = [Message::user("recent1"), Message::assistant("recent2")];
    let entries: Vec<StoreEntry> = recent.iter().cloned().map(StoreEntry::Message).collect();
    store.append("conv-1", &entries).await.unwrap();

    let loaded = mem.load("conv-1").await.unwrap();

    // The marker summary must NOT be prepended as a system message.
    // CachedMemory::load() returns the raw messages so the CompactingMemory
    // wrapper can apply its own policy.
    assert!(
        !loaded.iter().any(|m| matches!(m, Message::System { .. })),
        "load() must not prepend a marker summary as a system message, got: {loaded:?}"
    );
    // All raw messages (both pre- and post-marker) are preserved.
    assert_eq!(loaded.len(), 4);
    assert_msg_eq(&loaded[0], &old[0]);
    assert_msg_eq(&loaded[1], &old[1]);
    assert_msg_eq(&loaded[2], &recent[0]);
    assert_msg_eq(&loaded[3], &recent[1]);
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

    let marker = CompactionMarker::new("Summary".to_string(), Utc::now());
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

// ---------------------------------------------------------------------------
// Tool-verdict stamping: record → append stamps → consume-once
// ---------------------------------------------------------------------------

/// A recorded verdict must be stamped as `nu_agent_success` onto the first
/// Text block of the matching persisted ToolResult.
#[tokio::test]
async fn append_stamps_recorded_true_verdict() -> Result<()> {
    // -- Setup & Fixtures
    let tmp = TempDir::new().map_err(|e| format!("tempdir: {e:?}"))?;
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));
    mem.record_tool_verdict("tc1", true);

    // -- Exec
    mem.append(
        "conv-verdict-true",
        vec![tool_result_message("tc1", "all good")],
    )
    .await
    .map_err(|e| format!("append: {e:?}"))?;
    let entries = mem
        .load_all("conv-verdict-true")
        .await
        .map_err(|e| format!("load_all: {e:?}"))?;

    // -- Check
    assert_eq!(entries.len(), 1, "one appended message expected");
    assert_eq!(
        tool_result_flag(&entries[0]),
        Some(true),
        "recorded verdict must be stamped onto the persisted ToolResult"
    );
    Ok(())
}

/// A recorded `false` verdict must stamp `nu_agent_success` = false.
#[tokio::test]
async fn append_stamps_recorded_false_verdict() -> Result<()> {
    // -- Setup & Fixtures
    let tmp = TempDir::new().map_err(|e| format!("tempdir: {e:?}"))?;
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));
    mem.record_tool_verdict("tc1", false);

    // -- Exec
    mem.append(
        "conv-verdict-false",
        vec![tool_result_message("tc1", "the tool failed")],
    )
    .await
    .map_err(|e| format!("append: {e:?}"))?;
    let entries = mem
        .load_all("conv-verdict-false")
        .await
        .map_err(|e| format!("load_all: {e:?}"))?;

    // -- Check
    assert_eq!(entries.len(), 1, "one appended message expected");
    assert_eq!(
        tool_result_flag(&entries[0]),
        Some(false),
        "recorded false verdict must be stamped onto the persisted ToolResult"
    );
    Ok(())
}

/// A verdict is consumed on stamp: a later append of another ToolResult with
/// the same call id must NOT be stamped again.
#[tokio::test]
async fn append_consumes_verdict_after_first_stamp() -> Result<()> {
    // -- Setup & Fixtures
    let tmp = TempDir::new().map_err(|e| format!("tempdir: {e:?}"))?;
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));
    mem.record_tool_verdict("tc1", true);

    // -- Exec
    mem.append(
        "conv-consume-once",
        vec![tool_result_message("tc1", "first")],
    )
    .await
    .map_err(|e| format!("append 1: {e:?}"))?;
    mem.append(
        "conv-consume-once",
        vec![tool_result_message("tc1", "second")],
    )
    .await
    .map_err(|e| format!("append 2: {e:?}"))?;
    let entries = mem
        .load_all("conv-consume-once")
        .await
        .map_err(|e| format!("load_all: {e:?}"))?;

    // -- Check
    assert_eq!(entries.len(), 2, "two appended messages expected");
    assert_eq!(
        tool_result_flag(&entries[0]),
        Some(true),
        "first append must carry the stamped verdict"
    );
    assert_eq!(
        tool_result_flag(&entries[1]),
        None,
        "verdict must be consumed — the second append must stay unstamped"
    );
    Ok(())
}

/// A ToolResult appended without a recorded verdict must stay unstamped.
#[tokio::test]
async fn append_without_recorded_verdict_leaves_tool_result_unstamped() -> Result<()> {
    // -- Setup & Fixtures
    let tmp = TempDir::new().map_err(|e| format!("tempdir: {e:?}"))?;
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));

    // -- Exec
    mem.append(
        "conv-unstamped",
        vec![tool_result_message("tc1", "plain output")],
    )
    .await
    .map_err(|e| format!("append: {e:?}"))?;
    let entries = mem
        .load_all("conv-unstamped")
        .await
        .map_err(|e| format!("load_all: {e:?}"))?;

    // -- Check
    assert_eq!(entries.len(), 1, "one appended message expected");
    assert_eq!(
        tool_result_flag(&entries[0]),
        None,
        "no recorded verdict — the persisted ToolResult must carry no flag"
    );
    Ok(())
}

/// One batched append — a single User message carrying TWO ToolResults with
/// two recorded verdicts — must stamp each ToolResult per its own call id.
#[tokio::test]
async fn append_stamps_batched_tool_results_per_call_id() -> Result<()> {
    // -- Setup & Fixtures
    let tmp = TempDir::new().map_err(|e| format!("tempdir: {e:?}"))?;
    let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));
    mem.record_tool_verdict("tc1", true);
    mem.record_tool_verdict("tc2", false);

    // -- Exec
    let batched = Message::User {
        content: vec![
            tool_result("tc1", "first ok"),
            tool_result("tc2", "second failed"),
        ],
    };
    mem.append("conv-batched", vec![batched])
        .await
        .map_err(|e| format!("append: {e:?}"))?;
    let entries = mem
        .load_all("conv-batched")
        .await
        .map_err(|e| format!("load_all: {e:?}"))?;

    // -- Check
    assert_eq!(entries.len(), 1, "one batched message expected");
    assert_eq!(
        tool_result_flags(&entries[0]),
        vec![Some(true), Some(false)],
        "each ToolResult in the batch must carry its own verdict"
    );
    Ok(())
}

// -- Test Support

/// Build a ToolResult for `call_id` with one Text block holding `text` (no
/// additional params).
fn tool_result(call_id: &str, text: &str) -> UserContent {
    UserContent::ToolResult(ToolResult {
        call: ToolCallId::new_or_mint(call_id),
        provider: None,
        name: "test_tool".into(),
        content: vec![ToolResultContent::Text(Text {
            text: text.to_string(),
            additional_params: None,
        })],
    })
}

/// Build a User message carrying one ToolResult for `call_id` with one Text
/// block holding `text` (no additional params).
fn tool_result_message(call_id: &str, text: &str) -> Message {
    Message::User {
        content: vec![tool_result(call_id, text)],
    }
}

/// Read the `nu_agent_success` boolean from the first Text block of the
/// first ToolResult in a store entry. `None` when absent or not a boolean.
fn tool_result_flag(entry: &StoreEntry) -> Option<bool> {
    let StoreEntry::Message(Message::User { content }) = entry else {
        return None;
    };
    let Some(UserContent::ToolResult(tr)) = content.first() else {
        return None;
    };
    let Some(ToolResultContent::Text(text)) = tr.content.first() else {
        return None;
    };
    text.additional_params
        .as_ref()
        .and_then(|params| params.get("nu_agent_success"))
        .and_then(serde_json::Value::as_bool)
}

/// Read the `nu_agent_success` boolean from the first Text block of EACH
/// ToolResult in a store entry, in order. `None` when absent or not a boolean.
fn tool_result_flags(entry: &StoreEntry) -> Vec<Option<bool>> {
    let StoreEntry::Message(Message::User { content }) = entry else {
        return Vec::new();
    };
    content
        .iter()
        .filter_map(|c| {
            let UserContent::ToolResult(tr) = c else {
                return None;
            };
            let Some(ToolResultContent::Text(text)) = tr.content.first() else {
                return None;
            };
            Some(
                text.additional_params
                    .as_ref()
                    .and_then(|params| params.get("nu_agent_success"))
                    .and_then(serde_json::Value::as_bool),
            )
        })
        .collect()
}
