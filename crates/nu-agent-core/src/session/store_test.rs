use super::store::{
    CompactionMarker, FsSessionStore, StoreEntry, extract_llm_context, validate_tool_call_adjacency,
};
use crate::session::{SessionStore, extract_title};
use crate::types::Message;
use chrono::Utc;
use tempfile::TempDir;

/// Test result alias. NOTE: this module has a pre-existing test
/// (`assistant_content_rejects_tagless_block`) that uses the 2-arg
/// `Result<T, E>` form, so we cannot define a 1-arg `Result<T>` alias here.
/// New tests use the fully-qualified form instead.
type TestResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Compare two messages via their serialized JSON form.
///
/// rig 0.38+ uses `#[serde(flatten)]` on `Text::additional_params`.
/// A round-trip through serde turns `None` into `Some(Object {})`,
/// which breaks `PartialEq` even though the two forms are semantically
/// identical. Serializing first normalizes both sides.
pub(crate) fn assert_msg_eq(left: &Message, right: &Message) {
    assert_eq!(
        serde_json::to_value(left).unwrap(),
        serde_json::to_value(right).unwrap(),
    );
}

pub(crate) fn assert_msgs_eq(left: &[Message], right: &[Message]) {
    assert_eq!(left.len(), right.len(), "message count mismatch");
    for (i, (l, r)) in left.iter().zip(right.iter()).enumerate() {
        assert_eq!(
            serde_json::to_value(l).unwrap(),
            serde_json::to_value(r).unwrap(),
            "message {i} mismatch",
        );
    }
}

// JsonlConversationStore tests

#[tokio::test]
async fn jsonl_store_round_trip() -> TestResult<()> {
    // RED: Write test for basic round-trip functionality
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    let messages = vec![
        Message::user("Hello"),
        Message::assistant("Hi there!"),
        Message::user("How are you?"),
    ];

    // Create session with messages
    store.create("test-session", &messages).await.unwrap();

    // Read them back
    let (_metadata, entries) = store
        .load("test-session")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;

    // Verify they match
    assert_eq!(entries.len(), messages.len());
    for (entry, msg) in entries.iter().zip(messages.iter()) {
        if let StoreEntry::Message(e) = entry {
            assert_msg_eq(e, msg);
        } else {
            panic!("Expected Message entry");
        }
    }
    Ok(())
}

#[tokio::test]
async fn jsonl_store_append_messages() -> TestResult<()> {
    // RED: Test appending messages to existing session
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    let initial_messages = vec![
        Message::user("First message"),
        Message::assistant("First response"),
    ];

    // Create session with initial messages
    store
        .create("test-session", &initial_messages)
        .await
        .unwrap();

    // Append more messages
    let additional_messages = [
        Message::user("Second message"),
        Message::assistant("Second response"),
    ];
    let entries: Vec<StoreEntry> = additional_messages
        .iter()
        .cloned()
        .map(StoreEntry::Message)
        .collect();
    store.append("test-session", &entries).await.unwrap();

    // Load and verify all messages are present
    let (_metadata, loaded) = store
        .load("test-session")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(loaded.len(), 4);
    if let StoreEntry::Message(m) = &loaded[0] {
        assert_msg_eq(m, &initial_messages[0]);
    }
    if let StoreEntry::Message(m) = &loaded[1] {
        assert_msg_eq(m, &initial_messages[1]);
    }
    if let StoreEntry::Message(m) = &loaded[2] {
        assert_msg_eq(m, &additional_messages[0]);
    }
    if let StoreEntry::Message(m) = &loaded[3] {
        assert_msg_eq(m, &additional_messages[1]);
    }
    Ok(())
}

#[tokio::test]
async fn jsonl_store_load_empty_returns_none() {
    // RED: Test loading non-existent session
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    // Load non-existent session
    let loaded = store.load("non-existent-session").await.unwrap();

    // Should return None, not error
    assert!(loaded.is_none());
}

#[tokio::test]
async fn jsonl_store_delete_removes_session() {
    // RED: Test deleting a session
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    let messages = vec![Message::user("Hello"), Message::assistant("Hi")];

    // Create session
    store.create("test-session", &messages).await.unwrap();

    // Delete the session
    store.delete("test-session").await.unwrap();

    // Load should return None
    let loaded = store.load("test-session").await.unwrap();
    assert!(loaded.is_none());
}

#[tokio::test]
async fn jsonl_store_handles_corrupt_lines() -> TestResult<()> {
    // RED: Test that corrupt lines are skipped with warning
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    // Write valid messages first
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

    // Load should skip corrupt line but return valid messages
    let (_metadata, entries) = store
        .load("test-session")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), 3); // 2 original + 1 after corrupt line
    if let StoreEntry::Message(m) = &entries[0] {
        assert_msg_eq(m, &messages[0]);
    }
    if let StoreEntry::Message(m) = &entries[1] {
        assert_msg_eq(m, &messages[1]);
    }
    if let StoreEntry::Message(m) = &entries[2] {
        assert_msg_eq(m, &Message::user("Valid message 2"));
    }
    Ok(())
}

#[tokio::test]
async fn jsonl_store_append_creates_session_if_missing() -> TestResult<()> {
    // RED: Test that append creates session if it doesn't exist
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    let entries = vec![StoreEntry::Message(Message::user(
        "First message in new session",
    ))];

    // Append to non-existent session (creates metadata header automatically)
    store.append("new-session", &entries).await.unwrap();

    // Load and verify
    let (_metadata, loaded) = store
        .load("new-session")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(loaded.len(), 1);
    if let StoreEntry::Message(m) = &loaded[0] {
        assert_msg_eq(m, &Message::user("First message in new session"));
    } else {
        panic!("Expected Message entry");
    }
    Ok(())
}

// --- CompactionMarker and StoreEntry tests ---

#[test]
fn compaction_marker_serde_roundtrip() {
    let marker = CompactionMarker::new("Summary of old messages".to_string(), Utc::now());

    let json = serde_json::to_string(&marker).unwrap();
    let deserialized: CompactionMarker = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.entry_type, "compaction_marker");
    assert_eq!(deserialized.summary, "Summary of old messages");
    assert_eq!(deserialized.created_at, marker.created_at);
}

/// A marker JSON without `created_at` deserializes with the default (epoch)
/// via `#[serde(default)]`.
#[test]
fn compaction_marker_old_json_without_created_at_defaults() -> TestResult<()> {
    // -- Setup & Fixtures
    let old_json = r#"{
        "type": "compaction_marker",
        "summary": "old summary"
    }"#;

    // -- Exec
    let marker: CompactionMarker = serde_json::from_str(old_json)?;

    // -- Check
    assert_eq!(marker.summary, "old summary");
    assert_eq!(marker.created_at, chrono::DateTime::<Utc>::UNIX_EPOCH);
    Ok(())
}

#[tokio::test]
async fn append_marker_writes_type_field_to_raw_jsonl() {
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    // Create session with a message first
    store
        .create("test-session", &[Message::user("Hello")])
        .await
        .unwrap();

    // Append the marker
    let marker = CompactionMarker::new("Summary".to_string(), Utc::now());
    store
        .append("test-session", &[StoreEntry::Marker(marker)])
        .await
        .unwrap();

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

#[tokio::test]
async fn append_marker_writes_to_store() -> TestResult<()> {
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    // Create session with some messages first
    let messages = vec![Message::user("Hello"), Message::assistant("Hi")];
    store.create("test-session", &messages).await.unwrap();

    // Append a marker
    let marker = CompactionMarker::new("Summary".to_string(), Utc::now());
    store
        .append("test-session", &[StoreEntry::Marker(marker)])
        .await
        .unwrap();

    // load should return messages + marker
    let (_metadata, entries) = store
        .load("test-session")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), 3);
    match &entries[2] {
        StoreEntry::Marker(m) => {
            assert_eq!(m.summary, "Summary");
        }
        _ => panic!("Expected marker as third entry"),
    }
    Ok(())
}

#[tokio::test]
async fn load_returns_messages_and_markers_in_order() -> TestResult<()> {
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    let initial = vec![Message::user("m1"), Message::assistant("r1")];
    store.create("test-session", &initial).await.unwrap();

    // Append a marker
    let marker = CompactionMarker::new("S1".to_string(), Utc::now());
    store
        .append("test-session", &[StoreEntry::Marker(marker)])
        .await
        .unwrap();

    // Append more messages after marker
    let follow_up: Vec<StoreEntry> = vec![
        StoreEntry::Message(Message::user("m2")),
        StoreEntry::Message(Message::assistant("r2")),
    ];
    store.append("test-session", &follow_up).await.unwrap();

    let (_metadata, entries) = store
        .load("test-session")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), 5); // 2 msgs + 1 marker + 2 msgs

    assert!(matches!(&entries[0], StoreEntry::Message(_)));
    assert!(matches!(&entries[1], StoreEntry::Message(_)));
    assert!(matches!(&entries[2], StoreEntry::Marker(_)));
    assert!(matches!(&entries[3], StoreEntry::Message(_)));
    assert!(matches!(&entries[4], StoreEntry::Message(_)));
    Ok(())
}

#[tokio::test]
async fn load_returns_all_entries_including_markers() -> TestResult<()> {
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    let messages = vec![Message::user("m1"), Message::assistant("r1")];
    store.create("test-session", &messages).await.unwrap();

    // Append a marker
    let marker = CompactionMarker::new("S".to_string(), Utc::now());
    store
        .append("test-session", &[StoreEntry::Marker(marker)])
        .await
        .unwrap();

    // Append another message after
    store
        .append("test-session", &[StoreEntry::Message(Message::user("m2"))])
        .await
        .unwrap();

    // load() returns all entries including markers
    let (_metadata, entries) = store
        .load("test-session")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), 4); // m1, r1, marker, m2
    assert!(matches!(&entries[0], StoreEntry::Message(_)));
    assert!(matches!(&entries[1], StoreEntry::Message(_)));
    assert!(matches!(&entries[2], StoreEntry::Marker(_)));
    assert!(matches!(&entries[3], StoreEntry::Message(_)));
    Ok(())
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

    let marker = CompactionMarker::new("Summary of first 6".to_string(), Utc::now());
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

    let marker1 = CompactionMarker::new("Summary1".to_string(), Utc::now());
    entries.push(StoreEntry::Marker(marker1));

    // 4 messages between markers (2 user/assistant pairs)
    for i in 0..2 {
        entries.push(StoreEntry::Message(Message::user(format!("mid{}", i))));
        entries.push(StoreEntry::Message(Message::assistant(format!(
            "midr{}",
            i
        ))));
    }

    let marker2 = CompactionMarker::new("Summary2".to_string(), Utc::now());
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
fn extract_llm_context_kept_messages_after_marker_correct() {
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
    let marker = CompactionMarker::new("S".to_string(), Utc::now());
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
    let marker1 = CompactionMarker::new("OldSummary".to_string(), Utc::now());
    entries.push(StoreEntry::Marker(marker1));

    // 2 messages between markers (1 user/assistant pair)
    entries.push(StoreEntry::Message(Message::user("b0".to_string())));
    entries.push(StoreEntry::Message(Message::assistant("br0".to_string())));

    // marker2, kept=4
    let marker2 = CompactionMarker::new("NewSummary".to_string(), Utc::now());
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
    let marker = CompactionMarker::new(String::new(), Utc::now());
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

// --- AssistantContent tagged serialization (rig 0.42.0) ---

/// rig 0.42.0 serializes `AssistantContent` with a `"type"` tag
/// (`#[serde(tag = "type", rename_all = "lowercase")]`). Verify a
/// `Message::Assistant` round-trips through serde with the tag intact.
#[test]
fn assistant_content_round_trips_with_type_tag() {
    use crate::types::{AssistantContent, ToolCall, ToolCallId, ToolFunction};

    let msg = crate::types::Message::Assistant {
        id: None,
        content: vec![
            AssistantContent::Text(crate::types::Text::new("hello")),
            AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint("call_1"),
                ToolFunction::new("some_tool".to_string(), serde_json::json!({"a": 1})),
            )),
        ],
    };

    // Serialize and verify the "type" tags are present on each content block.
    let value = serde_json::to_value(&msg).unwrap();
    let content = value["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    // `rename_all = "lowercase"` lowercases the variant name without inserting
    // separators, so `ToolCall` serializes as `"toolcall"`.
    assert_eq!(content[1]["type"], "toolcall");
    assert_eq!(content[1]["function"]["name"], "some_tool");

    // Deserialize back and verify exact equality.
    let round_tripped: crate::types::Message = serde_json::from_value(value).unwrap();
    assert_msg_eq(&round_tripped, &msg);
}

/// A bare tagless `{"text": ...}` block must NOT deserialize as
/// `AssistantContent` — 0.42.0 requires the `"type"` tag (no fallback).
#[test]
fn assistant_content_rejects_tagless_block() {
    let tagless = serde_json::json!({"text": "hello"});
    let result: Result<crate::types::AssistantContent, _> = serde_json::from_value(tagless);
    assert!(result.is_err(), "tagless block must be rejected");
}

// --- StoreEntry serialization tests ---

#[tokio::test]
async fn append_writes_message_to_jsonl() {
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());
    store.create("s1", &[Message::user("hi")]).await.unwrap();

    // Read raw file and verify JSON contains the message
    let content = std::fs::read_to_string(temp_dir.path().join("s1.jsonl")).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    // Line 0 = metadata, line 1 = message
    assert_eq!(lines.len(), 2);
    let data_line = lines[1];
    let value: serde_json::Value = serde_json::from_str(data_line).unwrap();
    assert_eq!(value["role"], "user");
}

#[tokio::test]
async fn append_marker_writes_marker_to_jsonl() {
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());
    store.create("s1", &[Message::user("hi")]).await.unwrap();
    let marker = CompactionMarker::new("summary".to_string(), Utc::now());
    store
        .append("s1", &[StoreEntry::Marker(marker)])
        .await
        .unwrap();

    let content = std::fs::read_to_string(temp_dir.path().join("s1.jsonl")).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    // Line 0 = metadata, line 1 = message, line 2 = marker
    assert_eq!(lines.len(), 3);
    let marker_line = lines[2];
    let value: serde_json::Value = serde_json::from_str(marker_line).unwrap();
    assert_eq!(value["type"], "compaction_marker");
}

#[tokio::test]
async fn load_returns_all_entries_after_multiple_appends() -> TestResult<()> {
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());
    store.create("s1", &[Message::user("hi")]).await.unwrap();
    store
        .append("s1", &[StoreEntry::Message(Message::assistant("hello"))])
        .await
        .unwrap();
    let (_metadata, entries) = store
        .load("s1")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), 2);
    Ok(())
}

#[tokio::test]
async fn create_then_load_round_trips() -> TestResult<()> {
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());
    let msgs = vec![Message::user("hi"), Message::assistant("there")];
    store.create("s1", &msgs).await.unwrap();
    let (_meta, entries) = store
        .load("s1")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), 2);
    Ok(())
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

// --- Post-compaction entry ordering tests ---

#[tokio::test]
async fn load_preserves_marker_and_post_marker_entries() -> TestResult<()> {
    // JSONL layout: [msg, marker, msg]
    // Verify load returns all entries in correct order
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());
    let session_id = "s_post_marker";

    // Pre-compaction message
    store
        .create(session_id, &[Message::user("before compaction")])
        .await
        .unwrap();

    // Compaction marker
    let marker = CompactionMarker::new("summary".to_string(), Utc::now());
    store
        .append(session_id, &[StoreEntry::Marker(marker)])
        .await
        .unwrap();

    // Post-compaction kept message
    store
        .append(
            session_id,
            &[StoreEntry::Message(Message::assistant("kept"))],
        )
        .await
        .unwrap();

    let (_metadata, entries) = store
        .load(session_id)
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), 3); // original msg + marker + kept msg
    assert!(matches!(&entries[0], StoreEntry::Message(_)));
    assert!(matches!(&entries[1], StoreEntry::Marker(_)));
    assert!(matches!(&entries[2], StoreEntry::Message(_)));
    Ok(())
}

#[tokio::test]
async fn load_preserves_multiple_post_compaction_entries() -> TestResult<()> {
    // JSONL layout: [msg, marker, msg, msg]
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());
    let session_id = "s_post_compact_turn";

    store
        .create(session_id, &[Message::user("pre-compact")])
        .await
        .unwrap();

    let marker = CompactionMarker::new("summary".to_string(), Utc::now());
    store
        .append(session_id, &[StoreEntry::Marker(marker)])
        .await
        .unwrap();

    store
        .append(
            session_id,
            &[StoreEntry::Message(Message::assistant("kept"))],
        )
        .await
        .unwrap();

    // First real LLM turn after compaction
    store
        .append(
            session_id,
            &[StoreEntry::Message(Message::user("post-compact turn"))],
        )
        .await
        .unwrap();

    let (_metadata, entries) = store
        .load(session_id)
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), 4);
    assert!(matches!(&entries[0], StoreEntry::Message(_)));
    assert!(matches!(&entries[1], StoreEntry::Marker(_)));
    assert!(matches!(&entries[2], StoreEntry::Message(_)));
    assert!(matches!(&entries[3], StoreEntry::Message(_)));
    Ok(())
}

// ================================================================
// validate_tool_call_adjacency — TDD RED phase
// ================================================================

fn make_tool_call_msg(id: &str) -> crate::types::Message {
    use crate::types::{AssistantContent, ToolCall, ToolCallId, ToolFunction};
    crate::types::Message::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(ToolCall::new(
            ToolCallId::new_or_mint(id),
            ToolFunction::new("some_tool".to_string(), serde_json::json!({})),
        ))],
    }
}

fn make_tool_result_msg(id: &str) -> crate::types::Message {
    use crate::types::{ToolCallId, ToolResult, ToolResultContent, UserContent};
    crate::types::Message::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: ToolCallId::new_or_mint(id),
            provider: None,
            name: "some_tool".into(),
            content: vec![ToolResultContent::text("ok")],
        })],
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

// ================================================================
// FsSessionStore trait implementation tests
// ================================================================

#[tokio::test]
async fn fs_store_list_returns_empty_for_empty_directory() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    // List sessions in empty directory
    let sessions = store
        .list()
        .await
        .expect("Failed to list sessions in empty directory");

    assert_eq!(
        sessions.len(),
        0,
        "Should return empty list for empty directory"
    );
}

#[tokio::test]
async fn fs_store_list_returns_created_sessions() {
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

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

    let all = store.list().await.unwrap();
    assert_eq!(all.len(), 3);
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
    if let StoreEntry::Message(m) = &entries[0] {
        assert!(matches!(m, Message::User { .. }));
    } else {
        panic!("Expected Message entry");
    }
    Ok(())
}

#[tokio::test]
async fn fs_store_delete_removes_file() {
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    store
        .create("delete-me", &[Message::user("bye")])
        .await
        .unwrap();
    assert!(store.load("delete-me").await.unwrap().is_some());

    store.delete("delete-me").await.unwrap();
    assert!(store.load("delete-me").await.unwrap().is_none());
}

// ================================================================
// Title extraction tests
// ================================================================

#[test]
fn title_extracted_from_first_user_message() {
    let messages = vec![
        Message::user("Hello, this is my first message to the agent"),
        Message::assistant("Hi there!"),
    ];
    let title = extract_title(&messages);
    assert_eq!(
        title,
        Some("Hello, this is my first message to the agent".to_string())
    );
}

#[test]
fn title_none_when_no_user_message() {
    let messages = vec![Message::assistant("Hi there!")];
    let title = extract_title(&messages);
    assert_eq!(title, None);
}

#[test]
fn title_truncated_at_80_chars() {
    let long_text = "a".repeat(100);
    let messages = vec![Message::user(long_text)];
    let title = extract_title(&messages);
    assert_eq!(title, Some("a".repeat(80)));
}

#[test]
fn title_truncated_at_word_boundary() {
    let long_text = "hello world ".to_string() + &"x".repeat(70) + " more text";
    let messages = vec![Message::user(long_text)];
    let title = extract_title(&messages);
    // Should truncate at the last space within 80 chars
    assert!(title.as_ref().unwrap().len() <= 80);
    // "hello world " is 12 chars, last space at index 11 → "hello world" (11 chars)
    assert_eq!(title, Some("hello world".to_string()));
}

#[test]
fn title_none_when_empty_user_message() {
    let messages = vec![Message::user("   ")];
    let title = extract_title(&messages);
    assert_eq!(title, None);
}

#[tokio::test]
async fn title_survives_round_trip_fs_store() -> TestResult<()> {
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    let messages = vec![
        Message::user("My first message to the agent"),
        Message::assistant("Hello! How can I help?"),
    ];

    store.create("title-test", &messages).await.unwrap();

    // Verify via list
    let sessions = store.list().await.unwrap();
    let session = sessions.iter().find(|s| s.id == "title-test").unwrap();
    assert_eq!(
        session.title,
        Some("My first message to the agent".to_string())
    );

    // Verify via load
    let (metadata, _entries) = store
        .load("title-test")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(
        metadata.title,
        Some("My first message to the agent".to_string())
    );
    Ok(())
}

#[tokio::test]
async fn title_none_when_no_user_message_fs_store() -> TestResult<()> {
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    // Only assistant messages
    let messages = vec![Message::assistant("Hello!")];
    store.create("no-user-msg", &messages).await.unwrap();

    let sessions = store.list().await.unwrap();
    let session = sessions.iter().find(|s| s.id == "no-user-msg").unwrap();
    assert_eq!(session.title, None);

    let (metadata, _entries) = store
        .load("no-user-msg")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(metadata.title, None);
    Ok(())
}
