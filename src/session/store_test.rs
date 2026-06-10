use super::store::{
    CompactionMarker, ConversationStore, JsonlConversationStore, StoreEntry, extract_llm_context,
};
use rig::completion::Message;
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
    fn load(&self, _session_id: &str) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
        Ok(vec![])
    }

    fn append(
        &self,
        _session_id: &str,
        _messages: &[Message],
        _cumulative_tokens: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn clear(&self, _session_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn append_marker(
        &self,
        _session_id: &str,
        _marker: &CompactionMarker,
        _cumulative_tokens: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn load_all(
        &self,
        _session_id: &str,
    ) -> Result<(Vec<StoreEntry>, Option<u64>), Box<dyn std::error::Error>> {
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
    store.append("test-session", &[msg], 0).unwrap();

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
    store.append("test-session", &messages, 0).unwrap();

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
    store.append("test-session", &initial_messages, 0).unwrap();

    // Append more messages
    let additional_messages = vec![
        Message::user("Second message"),
        Message::assistant("Second response"),
    ];
    store.append("test-session", &additional_messages, 0).unwrap();

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
    store.append("test-session", &messages, 0).unwrap();

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
    store.append("test-session", &messages, 0).unwrap();

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
    store.append("new-session", &messages, 0).unwrap();

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
fn append_marker_writes_to_store() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    // Create session with some messages first
    let messages = vec![Message::user("Hello"), Message::assistant("Hi")];
    store.append("test-session", &messages, 0).unwrap();

    // Append a marker
    let marker = CompactionMarker::new("Summary".to_string(), 2, 5, "sliding_summary");
    store.append_marker("test-session", &marker, 0).unwrap();

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
    store.append("test-session", &initial, 0).unwrap();

    // Append a marker
    let marker = CompactionMarker::new("S1".to_string(), 2, 3, "sliding_summary");
    store.append_marker("test-session", &marker, 0).unwrap();

    // Append more messages after marker
    let follow_up = vec![Message::user("m2"), Message::assistant("r2")];
    store.append("test-session", &follow_up, 0).unwrap();

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
    store.append("test-session", &messages, 0).unwrap();

    // Append a marker
    let marker = CompactionMarker::new("S".to_string(), 1, 2, "sliding_summary");
    store.append_marker("test-session", &marker, 0).unwrap();

    // Append another message after
    store
        .append("test-session", &[Message::user("m2")], 0)
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
    ];

    let context = extract_llm_context(&entries);
    assert_eq!(context.len(), 3);
}

#[test]
fn extract_llm_context_single_marker() {
    // 7 messages + marker(kept=3) + 3 msgs after marker → summary + 3 post-marker = 4 messages
    let mut entries: Vec<StoreEntry> = (0..7)
        .map(|i| StoreEntry::Message(Message::user(format!("msg{}", i))))
        .collect();

    let marker = CompactionMarker::new("Summary of first 7".to_string(), 3, 7, "sliding_summary");
    entries.push(StoreEntry::Marker(marker));

    // 3 kept messages re-appended after marker
    for i in 7..10 {
        entries.push(StoreEntry::Message(Message::user(format!("msg{}", i))));
    }

    let context = extract_llm_context(&entries);
    assert_eq!(context.len(), 4); // 1 summary system msg + 3 post-marker

    // First is system summary
    match &context[0] {
        Message::System { content } => assert_eq!(content, "Summary of first 7"),
        _ => panic!("Expected system message with summary"),
    }
}

#[test]
fn extract_llm_context_multiple_markers() {
    // msgs + marker1 + msgs + marker2(kept=2) + 2 msgs after marker2 → uses marker2
    let mut entries = Vec::new();

    // 5 messages before marker1
    for i in 0..5 {
        entries.push(StoreEntry::Message(Message::user(format!("old{}", i))));
    }

    let marker1 = CompactionMarker::new("Summary1".to_string(), 3, 5, "sliding_summary");
    entries.push(StoreEntry::Marker(marker1));

    // 4 messages between markers
    for i in 0..4 {
        entries.push(StoreEntry::Message(Message::user(format!("mid{}", i))));
    }

    let marker2 = CompactionMarker::new("Summary2".to_string(), 2, 8, "sliding_summary");
    entries.push(StoreEntry::Marker(marker2));

    // 2 kept messages re-appended after marker2
    entries.push(StoreEntry::Message(Message::user("post0".to_string())));
    entries.push(StoreEntry::Message(Message::user("post1".to_string())));

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

    // 4 messages (old, before marker)
    for i in 0..4 {
        entries.push(StoreEntry::Message(Message::user(format!("m{}", i))));
    }

    // marker with kept=4
    let marker = CompactionMarker::new("S".to_string(), 4, 4, "sliding_summary");
    entries.push(StoreEntry::Marker(marker));

    // 4 kept messages re-appended after marker
    for i in 4..8 {
        entries.push(StoreEntry::Message(Message::user(format!("m{}", i))));
    }

    let context = extract_llm_context(&entries);
    // summary + 4 post-marker = 5
    assert_eq!(context.len(), 5);

    // Verify the kept messages are the 4 after marker (m4, m5, m6, m7)
    let kept_texts: Vec<String> = context[1..]
        .iter()
        .map(|m| serde_json::to_string(m).unwrap())
        .collect();
    for (i, text) in kept_texts.iter().enumerate() {
        let expected = format!("m{}", i + 4);
        assert!(
            text.contains(&expected),
            "Expected '{}' in kept message {}, got: {}",
            expected,
            i,
            text
        );
    }
}

#[test]
fn extract_llm_context_skips_older_markers_in_kept_range() {
    // Older markers are before the latest marker; kept messages are after.
    // Only the latest marker is used for context extraction.
    let mut entries = Vec::new();

    // 3 messages
    for i in 0..3 {
        entries.push(StoreEntry::Message(Message::user(format!("a{}", i))));
    }

    // marker1 at index 3
    let marker1 = CompactionMarker::new("OldSummary".to_string(), 2, 3, "sliding_summary");
    entries.push(StoreEntry::Marker(marker1));

    // 2 messages at indices 4, 5
    entries.push(StoreEntry::Message(Message::user("b0".to_string())));
    entries.push(StoreEntry::Message(Message::user("b1".to_string())));

    // marker2 at index 6, kept=3
    let marker2 = CompactionMarker::new("NewSummary".to_string(), 3, 5, "sliding_summary");
    entries.push(StoreEntry::Marker(marker2));

    // 3 kept messages re-appended after marker2
    entries.push(StoreEntry::Message(Message::user("k0".to_string())));
    entries.push(StoreEntry::Message(Message::user("k1".to_string())));
    entries.push(StoreEntry::Message(Message::user("k2".to_string())));

    let context = extract_llm_context(&entries);

    // summary + 3 post-marker messages = 4
    assert_eq!(context.len(), 4);

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

    for i in 0..3 {
        entries.push(StoreEntry::Message(Message::user(format!("w{}", i))));
    }

    // Marker with empty summary (SlidingWindow style)
    let marker = CompactionMarker::new(String::new(), 3, 3, "sliding_window");
    entries.push(StoreEntry::Marker(marker));

    // 3 kept messages re-appended after marker
    for i in 3..6 {
        entries.push(StoreEntry::Message(Message::user(format!("w{}", i))));
    }

    let context = extract_llm_context(&entries);

    // No summary + 3 post-marker = 3
    assert_eq!(context.len(), 3);

    // First should be user message (w3), not a system message
    assert!(
        matches!(&context[0], Message::User { .. }),
        "Expected user message, not system message when summary is empty"
    );
}

// --- cumulative_tokens I/O boundary tests ---

#[test]
fn append_writes_cumulative_tokens_to_json_lines() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    store
        .append("s1", &[Message::user("hi")], 500)
        .unwrap();

    // Read raw file and parse JSON lines
    let raw = std::fs::read_to_string(temp_dir.path().join("s1.jsonl")).unwrap();
    let lines: Vec<&str> = raw.lines().collect();
    // Line 0 = metadata, Line 1 = message
    assert!(lines.len() >= 2, "Expected at least 2 lines");
    let value: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(
        value.get("cumulative_tokens").and_then(|v| v.as_u64()),
        Some(500),
        "cumulative_tokens should be 500 in the JSON line"
    );
}

#[test]
fn append_marker_writes_cumulative_tokens_to_json() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    // Create session first
    store.append("s1", &[Message::user("hi")], 0).unwrap();

    let marker = CompactionMarker::new("Summary".to_string(), 1, 5, "sliding_summary");
    store.append_marker("s1", &marker, 1200).unwrap();

    // Read raw file and parse the marker line
    let raw = std::fs::read_to_string(temp_dir.path().join("s1.jsonl")).unwrap();
    let lines: Vec<&str> = raw.lines().collect();
    // Line 0 = metadata, Line 1 = message, Line 2 = marker
    assert!(lines.len() >= 3, "Expected at least 3 lines");
    let value: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(
        value.get("cumulative_tokens").and_then(|v| v.as_u64()),
        Some(1200),
        "cumulative_tokens should be 1200 in the marker JSON line"
    );
}

#[test]
fn load_all_returns_last_cumulative_tokens() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    store
        .append("s1", &[Message::user("m1")], 100)
        .unwrap();
    store
        .append("s1", &[Message::user("m2")], 350)
        .unwrap();

    let (entries, cumulative) = store.load_all("s1").unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(cumulative, Some(350));
}

#[test]
fn load_all_returns_none_for_legacy_entries() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    // Write raw JSON without cumulative_tokens field (legacy format)
    let metadata = serde_json::json!({
        "type": "session",
        "session_id": "s1",
        "created_at": "2024-01-01T00:00:00Z",
        "compaction_count": 0
    });
    let msg = serde_json::json!({
        "role": "user",
        "content": [{"type": "text", "text": "hello"}]
    });
    let content = format!("{}\n{}\n", metadata, msg);
    std::fs::write(temp_dir.path().join("s1.jsonl"), content).unwrap();

    let (entries, cumulative) = store.load_all("s1").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(cumulative, None, "Legacy entries should return None for cumulative");
}
