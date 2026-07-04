use super::store::{
    CompactionMarker, ConversationStore, JsonlConversationStore, StoreEntry, extract_llm_context,
    validate_tool_call_adjacency,
};
use crate::types::Message;
use tempfile::TempDir;

/// Compare two messages via their serialized JSON form.
///
/// rig 0.38+ uses `#[serde(flatten)]` on `Text::additional_params`.
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

/// Mock implementation for testing the trait interface
struct MockStore {
    _temp_dir: TempDir,
}

impl MockStore {
    fn new() -> Self {
        Self {
            _temp_dir: TempDir::new().unwrap(),
        }
    }
}

impl ConversationStore for MockStore {
    type Error = std::io::Error;

    fn load(&self, _session_id: &str) -> Result<Vec<Message>, std::io::Error> {
        Ok(vec![])
    }

    fn append(
        &self,
        _session_id: &str,
        _messages: &[Message],
        _last_total_tokens: Option<u64>,
    ) -> Result<(), std::io::Error> {
        Ok(())
    }

    fn clear(&self, _session_id: &str) -> Result<(), std::io::Error> {
        Ok(())
    }

    fn append_marker(
        &self,
        _session_id: &str,
        _marker: &CompactionMarker,
        _last_total_tokens: Option<u64>,
    ) -> Result<(), std::io::Error> {
        Ok(())
    }

    fn load_all(
        &self,
        _session_id: &str,
    ) -> Result<(Vec<StoreEntry>, Option<u64>), std::io::Error> {
        Ok((vec![], None))
    }
}

#[test]
fn trait_can_be_implemented() {
    // This test verifies that the trait compiles and can be implemented
    let store = MockStore::new();

    // Test load
    let messages = store.load("test-session").unwrap();
    assert_eq!(messages.len(), 0);

    // Test append
    let msg = Message::user("test");
    store.append("test-session", &[msg], None).unwrap();

    // Test clear
    store.clear("test-session").unwrap();
}

#[test]
fn trait_works_with_generic_bounds() {
    // This test verifies static dispatch works
    fn generic_store_fn<T: ConversationStore>(store: &T, session_id: &str) {
        let _ = store.load(session_id);
    }

    let store = MockStore::new();
    generic_store_fn(&store, "test-session");
}

// JsonlConversationStore tests

#[test]
fn jsonl_store_round_trip() {
    // RED: Write test for basic round-trip functionality
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    let messages = vec![
        Message::user("Hello"),
        Message::assistant("Hi there!"),
        Message::user("How are you?"),
    ];

    // Write messages
    store.append("test-session", &messages, None).unwrap();

    // Read them back
    let loaded = store.load("test-session").unwrap();

    // Verify they match
    assert_eq!(loaded.len(), messages.len());
    assert_msgs_eq(&loaded, &messages);
}

#[test]
fn jsonl_store_append_messages() {
    // RED: Test appending messages to existing session
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    let initial_messages = vec![
        Message::user("First message"),
        Message::assistant("First response"),
    ];

    // Write initial messages
    store
        .append("test-session", &initial_messages, None)
        .unwrap();

    // Append more messages
    let additional_messages = vec![
        Message::user("Second message"),
        Message::assistant("Second response"),
    ];
    store
        .append("test-session", &additional_messages, None)
        .unwrap();

    // Load and verify all messages are present
    let loaded = store.load("test-session").unwrap();
    assert_eq!(loaded.len(), 4);
    assert_msg_eq(&loaded[0], &initial_messages[0]);
    assert_msg_eq(&loaded[1], &initial_messages[1]);
    assert_msg_eq(&loaded[2], &additional_messages[0]);
    assert_msg_eq(&loaded[3], &additional_messages[1]);
}

#[test]
fn jsonl_store_load_empty_returns_empty_vec() {
    // RED: Test loading non-existent session
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    // Load non-existent session
    let loaded = store.load("non-existent-session").unwrap();

    // Should return empty vec, not error
    assert_eq!(loaded.len(), 0);
}

#[test]
fn jsonl_store_clear_removes_session() {
    // RED: Test clearing a session
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    let messages = vec![Message::user("Hello"), Message::assistant("Hi")];

    // Write messages
    store.append("test-session", &messages, None).unwrap();

    // Clear the session
    store.clear("test-session").unwrap();

    // Load should return empty
    let loaded = store.load("test-session").unwrap();
    assert_eq!(loaded.len(), 0);
}

#[test]
fn jsonl_store_handles_corrupt_lines() {
    // RED: Test that corrupt lines are skipped with warning
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    // Write valid messages first
    let messages = vec![
        Message::user("Valid message 1"),
        Message::assistant("Valid response 1"),
    ];
    store.append("test-session", &messages, None).unwrap();

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

    // Load should skip corrupt line but return valid messages
    let loaded = store.load("test-session").unwrap();
    assert_eq!(loaded.len(), 3); // 2 original + 1 after corrupt line
    assert_msg_eq(&loaded[0], &messages[0]);
    assert_msg_eq(&loaded[1], &messages[1]);
    assert_msg_eq(&loaded[2], &Message::user("Valid message 2"));
}

#[test]
fn jsonl_store_append_creates_session_if_missing() {
    // RED: Test that append creates session if it doesn't exist
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    let messages = vec![Message::user("First message in new session")];

    // Append to non-existent session
    store.append("new-session", &messages, None).unwrap();

    // Load and verify
    let loaded = store.load("new-session").unwrap();
    assert_eq!(loaded.len(), 1);
    assert_msg_eq(&loaded[0], &messages[0]);
}

// --- CompactionMarker and StoreEntry tests ---

#[test]
fn compaction_marker_serde_roundtrip() {
    let marker = CompactionMarker::new(
        "Summary of old messages".to_string(),
        5,
        20,
        "sliding_summary",
    );

    let json = serde_json::to_string(&marker).unwrap();
    let deserialized: CompactionMarker = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.entry_type, "compaction_marker");
    assert_eq!(deserialized.summary, "Summary of old messages");
    assert_eq!(deserialized.kept_recent_count, 5);
    assert_eq!(deserialized.summarized_count, 20);
    assert_eq!(deserialized.strategy, "sliding_summary");
    assert_eq!(deserialized.created_at, marker.created_at);
}

#[test]
fn append_marker_writes_type_field_to_raw_jsonl() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    // Append a message first so the metadata line is written
    store
        .append("test-session", &[Message::user("Hello")], None)
        .unwrap();

    // Append the marker
    let marker = CompactionMarker::new("Summary".to_string(), 1, 3, "sliding_summary");
    store.append_marker("test-session", &marker, None).unwrap();

    // Read raw JSONL bytes from disk
    let content = std::fs::read_to_string(temp_dir.path().join("test-session.jsonl")).unwrap();
    let lines: Vec<&str> = content.lines().collect();

    // Line 0 = metadata, line 1 = message, line 2 = marker
    let marker_line = lines[2];
    let value: serde_json::Value = serde_json::from_str(marker_line).unwrap();

    assert_eq!(
        value["type"].as_str(),
        Some("compaction_marker"),
        "raw JSONL must contain \"type\":\"compaction_marker\", got: {value}"
    );
}

#[test]
fn append_marker_writes_to_store() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    // Create session with some messages first
    let messages = vec![Message::user("Hello"), Message::assistant("Hi")];
    store.append("test-session", &messages, None).unwrap();

    // Append a marker
    let marker = CompactionMarker::new("Summary".to_string(), 2, 5, "sliding_summary");
    store.append_marker("test-session", &marker, None).unwrap();

    // load_all should return messages + marker
    let (entries, _) = store.load_all("test-session").unwrap();
    assert_eq!(entries.len(), 3);
    match &entries[2] {
        StoreEntry::Marker(m) => {
            assert_eq!(m.summary, "Summary");
            assert_eq!(m.kept_recent_count, 2);
            assert_eq!(m.summarized_count, 5);
            assert_eq!(m.strategy, "sliding_summary");
        }
        _ => panic!("Expected marker as third entry"),
    }
}

#[test]
fn load_all_returns_messages_and_markers_in_order() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    let initial = vec![Message::user("m1"), Message::assistant("r1")];
    store.append("test-session", &initial, None).unwrap();

    // Append a marker
    let marker = CompactionMarker::new("S1".to_string(), 2, 3, "sliding_summary");
    store.append_marker("test-session", &marker, None).unwrap();

    // Append more messages after marker
    let follow_up = vec![Message::user("m2"), Message::assistant("r2")];
    store.append("test-session", &follow_up, None).unwrap();

    let (entries, _) = store.load_all("test-session").unwrap();
    assert_eq!(entries.len(), 5); // 2 msgs + 1 marker + 2 msgs

    assert!(matches!(&entries[0], StoreEntry::Message(_)));
    assert!(matches!(&entries[1], StoreEntry::Message(_)));
    assert!(matches!(&entries[2], StoreEntry::Marker(_)));
    assert!(matches!(&entries[3], StoreEntry::Message(_)));
    assert!(matches!(&entries[4], StoreEntry::Message(_)));
}

#[test]
fn load_still_returns_only_messages() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    let messages = vec![Message::user("m1"), Message::assistant("r1")];
    store.append("test-session", &messages, None).unwrap();

    // Append a marker
    let marker = CompactionMarker::new("S".to_string(), 1, 2, "sliding_summary");
    store.append_marker("test-session", &marker, None).unwrap();

    // Append another message after
    store
        .append("test-session", &[Message::user("m2")], None)
        .unwrap();

    // load() returns only Messages, not markers (backward compat)
    let loaded = store.load("test-session").unwrap();
    assert_eq!(loaded.len(), 3); // m1, r1, m2 — marker is skipped
}

#[test]
fn extract_llm_context_no_markers() {
    let entries = vec![
        StoreEntry::Message(Message::user("Hello")),
        StoreEntry::Message(Message::assistant("Hi")),
        StoreEntry::Message(Message::user("How are you?")),
        StoreEntry::Message(Message::assistant("I am fine")),
    ];

    let context = extract_llm_context(&entries);
    assert_eq!(context.len(), 4);
}

#[test]
fn extract_llm_context_single_marker() {
    // 6 messages (3 user/assistant pairs) + marker(kept=4) + 4 msgs after marker → summary + 4 post-marker = 5 messages
    let mut entries: Vec<StoreEntry> = (0..3)
        .flat_map(|i| {
            [
                StoreEntry::Message(Message::user(format!("msg{}", i * 2))),
                StoreEntry::Message(Message::assistant(format!("reply{}", i * 2))),
            ]
        })
        .collect();

    let marker = CompactionMarker::new("Summary of first 6".to_string(), 4, 6, "sliding_summary");
    entries.push(StoreEntry::Marker(marker));

    // 4 kept messages re-appended after marker (2 user/assistant pairs)
    entries.push(StoreEntry::Message(Message::user("msg6".to_string())));
    entries.push(StoreEntry::Message(Message::assistant(
        "reply6".to_string(),
    )));
    entries.push(StoreEntry::Message(Message::user("msg8".to_string())));
    entries.push(StoreEntry::Message(Message::assistant(
        "reply8".to_string(),
    )));

    let context = extract_llm_context(&entries);
    assert_eq!(context.len(), 5); // 1 summary system msg + 4 post-marker

    // First is system summary
    match &context[0] {
        Message::System { content } => assert_eq!(content, "Summary of first 6"),
        _ => panic!("Expected system message with summary"),
    }
}

#[test]
fn extract_llm_context_multiple_markers() {
    // msgs + marker1 + msgs + marker2(kept=2) + 2 msgs after marker2 → uses marker2
    let mut entries = Vec::new();

    // 4 messages before marker1 (2 user/assistant pairs)
    for i in 0..2 {
        entries.push(StoreEntry::Message(Message::user(format!("old{}", i))));
        entries.push(StoreEntry::Message(Message::assistant(format!(
            "oldr{}",
            i
        ))));
    }

    let marker1 = CompactionMarker::new("Summary1".to_string(), 3, 5, "sliding_summary");
    entries.push(StoreEntry::Marker(marker1));

    // 4 messages between markers (2 user/assistant pairs)
    for i in 0..2 {
        entries.push(StoreEntry::Message(Message::user(format!("mid{}", i))));
        entries.push(StoreEntry::Message(Message::assistant(format!(
            "midr{}",
            i
        ))));
    }

    let marker2 = CompactionMarker::new("Summary2".to_string(), 2, 8, "sliding_summary");
    entries.push(StoreEntry::Marker(marker2));

    // 2 kept messages re-appended after marker2 (1 user/assistant pair)
    entries.push(StoreEntry::Message(Message::user("post0".to_string())));
    entries.push(StoreEntry::Message(Message::assistant(
        "postr0".to_string(),
    )));

    let context = extract_llm_context(&entries);

    // summary + 2 post-marker messages = 3
    assert_eq!(context.len(), 3);

    match &context[0] {
        Message::System { content } => assert_eq!(content, "Summary2"),
        _ => panic!("Expected system message with Summary2"),
    }
}

#[test]
fn extract_llm_context_kept_recent_count_correct() {
    // Verify exactly k messages after marker are included
    let mut entries = Vec::new();

    // 4 messages (old, before marker) — 2 user/assistant pairs
    for i in 0..2 {
        entries.push(StoreEntry::Message(Message::user(format!("m{}", i * 2))));
        entries.push(StoreEntry::Message(Message::assistant(format!(
            "r{}",
            i * 2
        ))));
    }

    // marker with kept=4
    let marker = CompactionMarker::new("S".to_string(), 4, 4, "sliding_summary");
    entries.push(StoreEntry::Marker(marker));

    // 4 kept messages re-appended after marker — 2 user/assistant pairs
    entries.push(StoreEntry::Message(Message::user("m4".to_string())));
    entries.push(StoreEntry::Message(Message::assistant("r4".to_string())));
    entries.push(StoreEntry::Message(Message::user("m6".to_string())));
    entries.push(StoreEntry::Message(Message::assistant("r6".to_string())));

    let context = extract_llm_context(&entries);
    // summary + 4 post-marker = 5
    assert_eq!(context.len(), 5);

    // Verify the kept messages are the 4 after marker
    let kept_texts: Vec<String> = context[1..]
        .iter()
        .map(|m| serde_json::to_string(m).unwrap())
        .collect();
    assert!(kept_texts[0].contains("m4"));
    assert!(kept_texts[1].contains("r4"));
    assert!(kept_texts[2].contains("m6"));
    assert!(kept_texts[3].contains("r6"));
}

#[test]
fn extract_llm_context_skips_older_markers_in_kept_range() {
    // Older markers are before the latest marker; kept messages are after.
    // Only the latest marker is used for context extraction.
    // 4 messages (2 user/assistant pairs)
    let mut entries = vec![
        StoreEntry::Message(Message::user("a0".to_string())),
        StoreEntry::Message(Message::assistant("ar0".to_string())),
        StoreEntry::Message(Message::user("a1".to_string())),
        StoreEntry::Message(Message::assistant("ar1".to_string())),
    ];

    // marker1 at index 4
    let marker1 = CompactionMarker::new("OldSummary".to_string(), 2, 3, "sliding_summary");
    entries.push(StoreEntry::Marker(marker1));

    // 2 messages between markers (1 user/assistant pair)
    entries.push(StoreEntry::Message(Message::user("b0".to_string())));
    entries.push(StoreEntry::Message(Message::assistant("br0".to_string())));

    // marker2, kept=4
    let marker2 = CompactionMarker::new("NewSummary".to_string(), 4, 6, "sliding_summary");
    entries.push(StoreEntry::Marker(marker2));

    // 4 kept messages re-appended after marker2 (2 user/assistant pairs)
    entries.push(StoreEntry::Message(Message::user("k0".to_string())));
    entries.push(StoreEntry::Message(Message::assistant("kr0".to_string())));
    entries.push(StoreEntry::Message(Message::user("k1".to_string())));
    entries.push(StoreEntry::Message(Message::assistant("kr1".to_string())));

    let context = extract_llm_context(&entries);

    // summary + 4 post-marker messages = 5
    assert_eq!(context.len(), 5);

    match &context[0] {
        Message::System { content } => assert_eq!(content, "NewSummary"),
        _ => panic!("Expected NewSummary system message"),
    }

    // Verify no system messages other than the first (i.e., OldSummary is not present)
    for msg in context.iter().skip(1) {
        assert!(
            !matches!(msg, Message::System { .. }),
            "Should not contain older marker summaries"
        );
    }
}

#[test]
fn extract_llm_context_empty_summary() {
    // SlidingWindow has empty summary → no system message prepended
    let mut entries = Vec::new();

    // 3 user/assistant pairs before the marker
    for i in 0..3 {
        entries.push(StoreEntry::Message(Message::user(format!("w{}", i))));
        entries.push(StoreEntry::Message(Message::assistant(format!("wr{}", i))));
    }

    // Marker with empty summary (SlidingWindow style)
    let marker = CompactionMarker::new(String::new(), 4, 6, "sliding_window");
    entries.push(StoreEntry::Marker(marker));

    // 4 kept messages re-appended after marker (2 user/assistant pairs)
    entries.push(StoreEntry::Message(Message::user("w6".to_string())));
    entries.push(StoreEntry::Message(Message::assistant("wr6".to_string())));
    entries.push(StoreEntry::Message(Message::user("w8".to_string())));
    entries.push(StoreEntry::Message(Message::assistant("wr8".to_string())));

    let context = extract_llm_context(&entries);

    // No summary + 4 post-marker = 4
    assert_eq!(context.len(), 4);

    // First should be user message (w6), not a system message
    assert!(
        matches!(&context[0], Message::User { .. }),
        "Expected user message, not system message when summary is empty"
    );
}

// --- last_total_tokens tests ---

#[test]
fn append_writes_last_total_tokens_to_json_lines() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    store
        .append("s1", &[Message::user("hi")], Some(1500))
        .unwrap();

    // Read raw file and verify JSON contains the field
    let content = std::fs::read_to_string(temp_dir.path().join("s1.jsonl")).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    // Skip metadata line (first line)
    let data_line = lines.last().unwrap();
    let value: serde_json::Value = serde_json::from_str(data_line).unwrap();
    assert_eq!(value["last_total_tokens"], 1500);
}

#[test]
fn append_marker_writes_last_total_tokens_to_json() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let marker = CompactionMarker::new("summary".to_string(), 5, 3, "sliding_summary");
    store.append_marker("s1", &marker, Some(2000)).unwrap();

    let content = std::fs::read_to_string(temp_dir.path().join("s1.jsonl")).unwrap();
    let data_line = content.lines().last().unwrap();
    let value: serde_json::Value = serde_json::from_str(data_line).unwrap();
    assert_eq!(value["last_total_tokens"], 2000);
}

#[test]
fn load_all_returns_last_total_tokens_from_last_entry() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    store
        .append("s1", &[Message::user("hi")], Some(100))
        .unwrap();
    store
        .append("s1", &[Message::assistant("hello")], Some(350))
        .unwrap();
    let (entries, last_tokens) = store.load_all("s1").unwrap();
    assert!(!entries.is_empty());
    assert_eq!(last_tokens, Some(350));
}

#[test]
fn load_all_returns_none_for_legacy_entries_without_tokens() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    // Write without tokens (legacy behavior)
    store.append("s1", &[Message::user("hi")], None).unwrap();
    let (entries, last_tokens) = store.load_all("s1").unwrap();
    assert!(!entries.is_empty());
    assert_eq!(last_tokens, None);
}

// --- Orphan user message trimming tests ---

#[test]
fn extract_llm_context_trims_trailing_orphan_user_message() {
    // [user, assistant, user(orphan)] → returns [user, assistant]
    let entries = vec![
        StoreEntry::Message(Message::user("first")),
        StoreEntry::Message(Message::assistant("reply")),
        StoreEntry::Message(Message::user("orphan")),
    ];

    let context = extract_llm_context(&entries);
    assert_eq!(context.len(), 2);
    assert!(matches!(&context[0], Message::User { .. }));
    assert!(matches!(&context[1], Message::Assistant { .. }));
}

#[test]
fn extract_llm_context_trims_multiple_stacked_orphan_user_messages() {
    // [user, assistant, user, user, user] → returns [user, assistant]
    let entries = vec![
        StoreEntry::Message(Message::user("first")),
        StoreEntry::Message(Message::assistant("reply")),
        StoreEntry::Message(Message::user("orphan1")),
        StoreEntry::Message(Message::user("orphan2")),
        StoreEntry::Message(Message::user("orphan3")),
    ];

    let context = extract_llm_context(&entries);
    assert_eq!(context.len(), 2);
    assert!(matches!(&context[0], Message::User { .. }));
    assert!(matches!(&context[1], Message::Assistant { .. }));
}

#[test]
fn extract_llm_context_preserves_valid_conversation_ending_with_assistant() {
    // [user, assistant, user, assistant] → unchanged (length 4)
    let entries = vec![
        StoreEntry::Message(Message::user("first")),
        StoreEntry::Message(Message::assistant("reply1")),
        StoreEntry::Message(Message::user("second")),
        StoreEntry::Message(Message::assistant("reply2")),
    ];

    let context = extract_llm_context(&entries);
    assert_eq!(context.len(), 4);
}

#[test]
fn extract_llm_context_empty_after_trimming_returns_empty() {
    // [user] only → returns []
    let entries = vec![StoreEntry::Message(Message::user("orphan"))];

    let context = extract_llm_context(&entries);
    assert_eq!(context.len(), 0);
}

// --- Post-compaction token tracking tests ---

#[test]
fn load_all_returns_none_when_post_marker_entries_have_null_tokens() {
    // JSONL layout: [msg(tokens=80000), marker(null), msg(null)]
    // Expected: load_all returns (entries, None) because all post-marker entries
    // have null tokens, and the marker resets the token tracking.
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let session_id = "s_post_marker_null";

    // Pre-compaction message with tokens
    store
        .append(
            session_id,
            &[Message::user("before compaction")],
            Some(80000),
        )
        .unwrap();

    // Compaction marker with null tokens
    let marker = CompactionMarker::new("summary".to_string(), 1, 1, "sliding_summary");
    store.append_marker(session_id, &marker, None).unwrap();

    // Post-compaction kept message with null tokens
    store
        .append(session_id, &[Message::assistant("kept")], None)
        .unwrap();

    let (entries, last_tokens) = store.load_all(session_id).unwrap();
    assert_eq!(entries.len(), 3); // original msg + marker + kept msg

    // Token tracking resets at the marker; null post-marker entries → None
    assert_eq!(
        last_tokens, None,
        "marker resets token tracking; null post-marker entries must yield None"
    );
}

#[test]
fn load_all_returns_fresh_tokens_after_post_compaction_turn() {
    // JSONL layout: [msg(80000), marker(null), msg(null), msg(5000)]
    // Expected: load_all returns (entries, Some(5000))
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let session_id = "s_post_compact_turn";

    store
        .append(session_id, &[Message::user("pre-compact")], Some(80000))
        .unwrap();

    let marker = CompactionMarker::new("summary".to_string(), 1, 1, "sliding_summary");
    store.append_marker(session_id, &marker, None).unwrap();

    store
        .append(session_id, &[Message::assistant("kept")], None)
        .unwrap();

    // First real LLM turn after compaction — this is the fresh count
    store
        .append(
            session_id,
            &[Message::user("post-compact turn")],
            Some(5000),
        )
        .unwrap();

    let (entries, last_tokens) = store.load_all(session_id).unwrap();
    assert_eq!(entries.len(), 4);
    assert_eq!(
        last_tokens,
        Some(5000),
        "first post-compaction turn with tokens should be returned"
    );
}

// ================================================================
// validate_tool_call_adjacency — TDD RED phase
// ================================================================

fn make_tool_call_msg(id: &str) -> crate::types::Message {
    use crate::types::{AssistantContent, ToolCall, ToolFunction};
    crate::types::Message::Assistant {
        id: None,
        content: rig::one_or_many::OneOrMany::one(AssistantContent::ToolCall(ToolCall::new(
            id.to_string(),
            ToolFunction::new("some_tool".to_string(), serde_json::json!({})),
        ))),
    }
}

fn make_tool_result_msg(id: &str) -> crate::types::Message {
    use crate::types::{ToolResult, ToolResultContent, UserContent};
    crate::types::Message::User {
        content: rig::one_or_many::OneOrMany::one(UserContent::ToolResult(ToolResult {
            id: id.to_string(),
            call_id: None,
            content: rig::one_or_many::OneOrMany::one(ToolResultContent::text("ok")),
        })),
    }
}

/// Test: structurally valid list (ToolCall at i, ToolResult at i+1) → returned unchanged.
#[test]
fn validate_tool_call_adjacency_passes_valid_adjacent_pair() {
    let msgs = vec![
        crate::types::Message::user("hi"),
        make_tool_call_msg("tc1"),
        make_tool_result_msg("tc1"),
    ];
    let expected = msgs.clone();
    let result = validate_tool_call_adjacency(msgs);
    assert_msgs_eq(&result, &expected);
}

/// Test: non-adjacent pair (ToolCall at i, ToolResult at i+2 with something else at i+1) →
/// both stripped, remaining messages returned.
#[test]
fn validate_tool_call_adjacency_strips_non_adjacent_pair() {
    let msgs = vec![
        crate::types::Message::user("start"),
        make_tool_call_msg("tc1"),
        crate::types::Message::user("in-between"),
        make_tool_result_msg("tc1"),
    ];
    let result = validate_tool_call_adjacency(msgs);
    // ToolCall and ToolResult must both be stripped; remaining: user("start"), user("in-between")
    let has_tc = result
        .iter()
        .any(|m| matches!(m, crate::types::Message::Assistant { .. }));
    let has_tr = result.iter().any(|m| match m {
        crate::types::Message::User { content } => content
            .iter()
            .any(|i| matches!(i, crate::types::UserContent::ToolResult(_))),
        _ => false,
    });
    assert!(!has_tc, "ToolCall must be stripped from non-adjacent pair");
    assert!(
        !has_tr,
        "ToolResult must be stripped from non-adjacent pair"
    );
    assert_eq!(result.len(), 2, "only the two non-tool messages remain");
}

/// Test: multiple valid tool call pairs in sequence → all pass through unchanged.
#[test]
fn validate_tool_call_adjacency_passes_multiple_valid_pairs() {
    let msgs = vec![
        crate::types::Message::user("go"),
        make_tool_call_msg("tc1"),
        make_tool_result_msg("tc1"),
        make_tool_call_msg("tc2"),
        make_tool_result_msg("tc2"),
        crate::types::Message::assistant("done"),
    ];
    let expected = msgs.clone();
    let result = validate_tool_call_adjacency(msgs);
    assert_msgs_eq(&result, &expected);
}
