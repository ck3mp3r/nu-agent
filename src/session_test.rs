use crate::session::SessionStore;
use std::fs;
use std::os::unix::fs::MetadataExt;
use tempfile::TempDir;

/// Test that get_or_create with None generates a session ID with correct format.
#[test]
fn test_get_or_create_auto_generates_id() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session = store.get_or_create(None).expect("Failed to create session");

    // Verify ID format: session-<timestamp>
    assert!(
        session.id().starts_with("session-"),
        "Session ID should start with 'session-', got: {}",
        session.id()
    );

    // Verify ID contains timestamp-like suffix (digits and dashes)
    let suffix = session.id().strip_prefix("session-").unwrap();
    assert!(
        !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit() || c == '-'),
        "Session ID suffix should contain only digits and dashes, got: {}",
        suffix
    );
}

/// Test that calling get_or_create with the same ID returns the same session.
#[test]
fn test_get_or_create_loads_existing_session() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-session-123".to_string();

    // First call creates the session
    let session1 = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    assert_eq!(session1.id(), &session_id);

    // Second call should load the same session
    let session2 = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to load session");

    assert_eq!(session2.id(), &session_id);
    assert_eq!(session1.id(), session2.id());
}

/// Test that get_or_create creates a JSONL file with proper format.
#[test]
fn test_get_or_create_creates_jsonl_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-jsonl-creation".to_string();

    let _session = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    // Verify JSONL file exists
    let jsonl_path = temp_dir.path().join(format!("{}.jsonl", session_id));
    assert!(
        jsonl_path.exists(),
        "JSONL file should exist at {:?}",
        jsonl_path
    );

    // Verify file format: first line is metadata JSON
    let content = fs::read_to_string(&jsonl_path).expect("Failed to read JSONL file");
    let lines: Vec<&str> = content.lines().collect();

    assert!(
        !lines.is_empty(),
        "JSONL file should have at least metadata line"
    );

    // Parse first line as metadata
    let metadata: serde_json::Value =
        serde_json::from_str(lines[0]).expect("First line should be valid JSON metadata");

    assert_eq!(
        metadata.get("type").and_then(|v| v.as_str()),
        Some("meta"),
        "Metadata should have type 'meta'"
    );

    assert_eq!(
        metadata.get("session_id").and_then(|v| v.as_str()),
        Some(session_id.as_str()),
        "Metadata should contain session_id"
    );
}

/// Test that multiple sessions with auto-generated IDs have unique IDs.
#[test]
fn test_auto_generated_ids_are_unique() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session1 = store
        .get_or_create(None)
        .expect("Failed to create session1");
    let session2 = store
        .get_or_create(None)
        .expect("Failed to create session2");

    assert_ne!(
        session1.id(),
        session2.id(),
        "Auto-generated session IDs should be unique"
    );
}

/// Test that append_message writes messages as JSONL lines.
#[test]
fn test_append_message_writes_jsonl() {
    use crate::session::Message;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-append-messages".to_string();
    let mut session = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    // Append 3 messages
    let msg1 = Message::new("user".to_string(), "Hello".to_string());
    let msg2 = Message::new("assistant".to_string(), "Hi there".to_string());
    let msg3 = Message::new("user".to_string(), "How are you?".to_string());

    session
        .append_message(&store, msg1)
        .expect("Failed to append message 1");
    session
        .append_message(&store, msg2)
        .expect("Failed to append message 2");
    session
        .append_message(&store, msg3)
        .expect("Failed to append message 3");

    // Read the JSONL file
    let jsonl_path = temp_dir.path().join(format!("{}.jsonl", session_id));
    let content = fs::read_to_string(&jsonl_path).expect("Failed to read JSONL file");
    let lines: Vec<&str> = content.lines().collect();

    // Verify: 1 metadata line + 3 message lines = 4 total
    assert_eq!(
        lines.len(),
        4,
        "Should have 4 lines (1 metadata + 3 messages)"
    );

    // Verify first line is metadata
    let metadata: serde_json::Value =
        serde_json::from_str(lines[0]).expect("First line should be valid JSON");
    assert_eq!(
        metadata.get("type").and_then(|v| v.as_str()),
        Some("meta"),
        "First line should be metadata"
    );

    // Verify each message line is valid JSON
    for (i, line) in lines.iter().skip(1).enumerate() {
        let msg: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|_| panic!("Message {} should be valid JSON: {}", i + 1, line));

        // Verify message has required fields
        assert!(
            msg.get("role").is_some(),
            "Message {} should have 'role' field",
            i + 1
        );
        assert!(
            msg.get("content").is_some(),
            "Message {} should have 'content' field",
            i + 1
        );
        assert!(
            msg.get("timestamp").is_some(),
            "Message {} should have 'timestamp' field",
            i + 1
        );
    }

    // Verify message content
    let msg1_json: serde_json::Value =
        serde_json::from_str(lines[1]).expect("Message 1 should be valid JSON");
    assert_eq!(msg1_json.get("role").and_then(|v| v.as_str()), Some("user"));
    assert_eq!(
        msg1_json.get("content").and_then(|v| v.as_str()),
        Some("Hello")
    );
}

/// Test that list_sessions returns correct metadata for multiple sessions.
#[test]
fn test_list_sessions_returns_correct_metadata() {
    use crate::session::Message;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    // Create 3 sessions with different numbers of messages
    let session1_id = "test-session-1".to_string();
    let mut session1 = store
        .get_or_create(Some(session1_id.clone()))
        .expect("Failed to create session1");

    session1
        .append_message(&store, Message::new("user".to_string(), "msg1".to_string()))
        .expect("Failed to append to session1");
    session1
        .append_message(
            &store,
            Message::new("assistant".to_string(), "msg2".to_string()),
        )
        .expect("Failed to append to session1");

    let session2_id = "test-session-2".to_string();
    let mut session2 = store
        .get_or_create(Some(session2_id.clone()))
        .expect("Failed to create session2");

    session2
        .append_message(&store, Message::new("user".to_string(), "msg1".to_string()))
        .expect("Failed to append to session2");

    let session3_id = "test-session-3".to_string();
    let _session3 = store
        .get_or_create(Some(session3_id.clone()))
        .expect("Failed to create session3");

    // List sessions
    let sessions = store.list_sessions().expect("Failed to list sessions");

    // Should have 3 sessions
    assert_eq!(sessions.len(), 3, "Should list 3 sessions");

    // Find each session in the list
    let s1 = sessions
        .iter()
        .find(|s| s.id == session1_id)
        .expect("Should find session1");
    let s2 = sessions
        .iter()
        .find(|s| s.id == session2_id)
        .expect("Should find session2");
    let s3 = sessions
        .iter()
        .find(|s| s.id == session3_id)
        .expect("Should find session3");

    // Verify message counts
    assert_eq!(s1.message_count, 2, "Session1 should have 2 messages");
    assert_eq!(s2.message_count, 1, "Session2 should have 1 message");
    assert_eq!(s3.message_count, 0, "Session3 should have 0 messages");

    // Verify all have compaction_count 0 (not implemented yet)
    assert_eq!(s1.compaction_count, 0);
    assert_eq!(s2.compaction_count, 0);
    assert_eq!(s3.compaction_count, 0);

    // Verify last_active is set (should be created_at for now)
    assert!(
        s1.last_active.timestamp() > 0,
        "Should have valid timestamp"
    );
    assert!(
        s2.last_active.timestamp() > 0,
        "Should have valid timestamp"
    );
    assert!(
        s3.last_active.timestamp() > 0,
        "Should have valid timestamp"
    );
}

/// Test that list_sessions returns empty vector for empty cache directory.
#[test]
fn test_list_sessions_empty_directory() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    // List sessions in empty directory
    let sessions = store
        .list_sessions()
        .expect("Failed to list sessions in empty directory");

    assert_eq!(
        sessions.len(),
        0,
        "Should return empty list for empty directory"
    );
}

/// Test that load_session reads messages from JSONL file.
#[test]
fn test_load_session_with_messages() {
    use crate::session::Message;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-load-messages".to_string();
    let mut session = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    // Append some messages
    let msg1 = Message::new("user".to_string(), "Hello".to_string());
    let msg2 = Message::new("assistant".to_string(), "Hi there".to_string());
    let msg3 = Message::new("user".to_string(), "How are you?".to_string());

    session
        .append_message(&store, msg1)
        .expect("Failed to append message 1");
    session
        .append_message(&store, msg2)
        .expect("Failed to append message 2");
    session
        .append_message(&store, msg3)
        .expect("Failed to append message 3");

    // Now load the session
    let loaded_session = store
        .load_session(&session_id)
        .expect("Failed to load session");

    // Verify session ID matches
    assert_eq!(loaded_session.id(), session_id);

    // Verify messages were loaded
    let messages = loaded_session.messages();
    assert_eq!(messages.len(), 3, "Should have loaded 3 messages");

    // Verify message content
    assert_eq!(messages[0].role(), "user");
    assert_eq!(messages[0].content(), "Hello");

    assert_eq!(messages[1].role(), "assistant");
    assert_eq!(messages[1].content(), "Hi there");

    assert_eq!(messages[2].role(), "user");
    assert_eq!(messages[2].content(), "How are you?");
}

/// Test that load_session handles empty session (metadata only).
#[test]
fn test_load_session_empty_messages() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-empty-session".to_string();
    let _session = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    // Load session without adding messages
    let loaded_session = store
        .load_session(&session_id)
        .expect("Failed to load session");

    assert_eq!(loaded_session.id(), session_id);
    assert_eq!(
        loaded_session.messages().len(),
        0,
        "Should have no messages"
    );
}

/// Test that load_session handles malformed JSONL.
#[test]
fn test_load_session_malformed_jsonl() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-malformed".to_string();

    // Create a malformed JSONL file manually
    let jsonl_path = temp_dir.path().join(format!("{}.jsonl", session_id));
    let malformed_content = r#"{"type":"meta","session_id":"test-malformed","created_at":"2024-01-01T00:00:00Z"}
{"role":"user","content":"valid message","timestamp":"2024-01-01T00:00:01Z"}
this is not valid json
{"role":"user","content":"another valid","timestamp":"2024-01-01T00:00:02Z"}"#;

    fs::write(&jsonl_path, malformed_content).expect("Failed to write malformed file");

    // Load should return an error
    let result = store.load_session(&session_id);
    assert!(
        result.is_err(),
        "Loading malformed JSONL should return an error"
    );

    // Verify error message mentions JSON parsing
    let error = result.unwrap_err();
    let error_msg = error.to_string().to_lowercase();
    assert!(
        error_msg.contains("json") || error_msg.contains("parse") || error_msg.contains("message"),
        "Error should mention JSON parsing issue, got: {}",
        error
    );
}

/// Test that add_message appends a message and updates the messages vector.
#[test]
fn test_add_message_appends_and_updates_vector() {
    use crate::session::Message;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-add-message".to_string();
    let mut session = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    // Verify session starts with 0 messages
    assert_eq!(session.messages().len(), 0);

    // Add first message
    let msg1 = Message::new("user".to_string(), "First message".to_string());
    session
        .add_message(&store, msg1)
        .expect("Failed to add message 1");

    // Verify messages vector is updated
    assert_eq!(session.messages().len(), 1);
    assert_eq!(session.messages()[0].role(), "user");
    assert_eq!(session.messages()[0].content(), "First message");

    // Add second message
    let msg2 = Message::new("assistant".to_string(), "Second message".to_string());
    session
        .add_message(&store, msg2)
        .expect("Failed to add message 2");

    // Verify messages vector is updated
    assert_eq!(session.messages().len(), 2);
    assert_eq!(session.messages()[1].role(), "assistant");
    assert_eq!(session.messages()[1].content(), "Second message");
}

/// Test that add_message triggers compaction when threshold is exceeded.
#[test]
fn test_add_message_triggers_compaction_on_threshold() {
    use crate::session::{Message, SessionConfig};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-compaction-trigger".to_string();
    let mut session = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    // Set compaction threshold to 3
    session.set_config(SessionConfig {
        compaction_threshold: 3,
        compaction_strategy: crate::session::CompactionStrategy::Truncate,
        keep_recent: 10,
    });

    // Add messages up to threshold
    session
        .add_message(&store, Message::new("user".to_string(), "msg1".to_string()))
        .expect("Failed to add message 1");
    session
        .add_message(
            &store,
            Message::new("assistant".to_string(), "msg2".to_string()),
        )
        .expect("Failed to add message 2");
    session
        .add_message(&store, Message::new("user".to_string(), "msg3".to_string()))
        .expect("Failed to add message 3");

    // At threshold, should have 3 messages
    assert_eq!(session.messages().len(), 3);

    // Add one more message to exceed threshold
    session
        .add_message(
            &store,
            Message::new("assistant".to_string(), "msg4".to_string()),
        )
        .expect("Failed to add message 4");

    // Should have triggered compaction (placeholder behavior for now)
    // For now, we just verify the method doesn't panic and still adds the message
    assert_eq!(session.messages().len(), 4);
}

/// Test that maybe_compact checks message count against threshold.
/// With threshold=5, adding 6 messages should trigger compaction.
#[test]
fn test_maybe_compact_triggers_on_threshold() {
    use crate::session::{Message, SessionConfig};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-maybe-compact".to_string();
    let mut session = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    // Set threshold to 5
    session.set_config(SessionConfig {
        compaction_threshold: 5,
        compaction_strategy: crate::session::CompactionStrategy::Truncate,
        keep_recent: 10,
    });

    // Add 6 messages (1 over threshold)
    for i in 0..6 {
        session
            .add_message(
                &store,
                Message::new("user".to_string(), format!("msg{}", i)),
            )
            .expect("Failed to add message");
    }

    // Call maybe_compact
    let compacted = session
        .maybe_compact(&store)
        .expect("maybe_compact should succeed");

    // Should have triggered compaction
    assert!(compacted, "Should have triggered compaction");
}

/// Test that maybe_compact does NOT trigger when under threshold.
#[test]
fn test_maybe_compact_does_not_trigger_under_threshold() {
    use crate::session::{Message, SessionConfig};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-no-compact".to_string();
    let mut session = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    // Set threshold to 10
    session.set_config(SessionConfig {
        compaction_threshold: 10,
        compaction_strategy: crate::session::CompactionStrategy::Truncate,
        keep_recent: 10,
    });

    // Add only 5 messages (well under threshold)
    for i in 0..5 {
        session
            .add_message(
                &store,
                Message::new("user".to_string(), format!("msg{}", i)),
            )
            .expect("Failed to add message");
    }

    // Call maybe_compact
    let compacted = session
        .maybe_compact(&store)
        .expect("maybe_compact should succeed");

    // Should NOT have triggered compaction
    assert!(!compacted, "Should not trigger compaction under threshold");
}

/// Test that maybe_compact works with Summarize strategy.
#[test]
fn test_maybe_compact_summarize_strategy() {
    use crate::session::{Message, SessionConfig};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-summarize".to_string();
    let mut session = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    session.set_config(SessionConfig {
        compaction_threshold: 3,
        compaction_strategy: crate::session::CompactionStrategy::Summarize,
        keep_recent: 10,
    });

    // Add 4 messages (over threshold)
    for i in 0..4 {
        session
            .add_message(
                &store,
                Message::new("user".to_string(), format!("msg{}", i)),
            )
            .expect("Failed to add message");
    }

    // Should succeed (even if strategy is stubbed)
    let compacted = session
        .maybe_compact(&store)
        .expect("maybe_compact should succeed");

    assert!(
        compacted,
        "Should trigger compaction with Summarize strategy"
    );
}

/// Test that maybe_compact works with Sliding strategy.
#[test]
fn test_maybe_compact_sliding_strategy() {
    use crate::session::{Message, SessionConfig};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-sliding".to_string();
    let mut session = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    session.set_config(SessionConfig {
        compaction_threshold: 3,
        compaction_strategy: crate::session::CompactionStrategy::Sliding,
        keep_recent: 10,
    });

    // Add 4 messages (over threshold)
    for i in 0..4 {
        session
            .add_message(
                &store,
                Message::new("user".to_string(), format!("msg{}", i)),
            )
            .expect("Failed to add message");
    }

    // Should succeed (even if strategy is stubbed)
    let compacted = session
        .maybe_compact(&store)
        .expect("maybe_compact should succeed");

    assert!(compacted, "Should trigger compaction with Sliding strategy");
}

/// Test that truncate_old() drops oldest messages beyond threshold, keeping last N.
/// Threshold=5, keep_recent=2, add 10 messages, verify only last 2 remain after compaction.
#[test]
fn test_compact_truncate_keeps_recent_messages() {
    use crate::session::{Message, SessionConfig};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-truncate".to_string();
    let mut session = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    // Configure: threshold=5, keep_recent=2
    session.set_config(SessionConfig {
        compaction_threshold: 5,
        compaction_strategy: crate::session::CompactionStrategy::Truncate,
        keep_recent: 2,
    });

    // Add 10 messages (well over threshold)
    for i in 0..10 {
        session
            .add_message(
                &store,
                Message::new("user".to_string(), format!("msg{}", i)),
            )
            .expect("Failed to add message");
    }

    // Verify we have 10 messages before compaction
    assert_eq!(session.messages().len(), 10);

    // Trigger compaction
    let compacted = session
        .maybe_compact(&store)
        .expect("Compaction should succeed");

    assert!(compacted, "Should have triggered compaction");

    // After compaction, should keep only last 2 messages (msg8, msg9)
    assert_eq!(
        session.messages().len(),
        2,
        "Should keep only last 2 messages after truncation"
    );

    // Verify the correct messages remain (last 2)
    assert_eq!(session.messages()[0].content(), "msg8");
    assert_eq!(session.messages()[1].content(), "msg9");

    // Reload session from disk to verify persistence
    let loaded_session = store
        .load_session(&session_id)
        .expect("Failed to reload session");

    assert_eq!(
        loaded_session.messages().len(),
        2,
        "Reloaded session should have 2 messages"
    );
    assert_eq!(loaded_session.messages()[0].content(), "msg8");
    assert_eq!(loaded_session.messages()[1].content(), "msg9");
}

/// Test sliding window compaction strategy.
/// With keep_recent=3, add 10 messages, verify only last 3 remain.
#[test]
fn test_compact_sliding_window_keeps_last_n_messages() {
    use crate::session::{Message, SessionConfig};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-sliding-window".to_string();
    let mut session = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    // Configure: threshold=5, keep_recent=3, strategy=Sliding
    session.set_config(SessionConfig {
        compaction_threshold: 5,
        compaction_strategy: crate::session::CompactionStrategy::Sliding,
        keep_recent: 3,
    });

    // Add 10 messages (well over threshold)
    for i in 0..10 {
        session
            .add_message(
                &store,
                Message::new("user".to_string(), format!("msg{}", i)),
            )
            .expect("Failed to add message");
    }

    // Verify we have 10 messages before compaction
    assert_eq!(session.messages().len(), 10);

    // Trigger compaction
    let compacted = session
        .maybe_compact(&store)
        .expect("Compaction should succeed");

    assert!(compacted, "Should have triggered compaction");

    // After compaction, should keep only last 3 messages (msg7, msg8, msg9)
    assert_eq!(
        session.messages().len(),
        3,
        "Should keep only last 3 messages after sliding window compaction"
    );

    // Verify the correct messages remain (last 3)
    assert_eq!(session.messages()[0].content(), "msg7");
    assert_eq!(session.messages()[1].content(), "msg8");
    assert_eq!(session.messages()[2].content(), "msg9");

    // Reload session from disk to verify persistence
    let loaded_session = store
        .load_session(&session_id)
        .expect("Failed to reload session");

    assert_eq!(
        loaded_session.messages().len(),
        3,
        "Reloaded session should have 3 messages"
    );
    assert_eq!(loaded_session.messages()[0].content(), "msg7");
    assert_eq!(loaded_session.messages()[1].content(), "msg8");
    assert_eq!(loaded_session.messages()[2].content(), "msg9");
}

/// Test that sliding window compaction correctly updates when adding more messages.
/// Verify window slides correctly as new messages are added.
#[test]
fn test_compact_sliding_window_slides_correctly() {
    use crate::session::{Message, SessionConfig};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-sliding-window-slides".to_string();
    let mut session = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    // Configure: threshold=3, keep_recent=3, strategy=Sliding
    session.set_config(SessionConfig {
        compaction_threshold: 3,
        compaction_strategy: crate::session::CompactionStrategy::Sliding,
        keep_recent: 3,
    });

    // Add 5 messages (over threshold)
    for i in 0..5 {
        session
            .add_message(
                &store,
                Message::new("user".to_string(), format!("msg{}", i)),
            )
            .expect("Failed to add message");
    }

    // Trigger first compaction
    let compacted = session
        .maybe_compact(&store)
        .expect("First compaction should succeed");

    assert!(compacted, "Should have triggered compaction");

    // Should keep last 3: msg2, msg3, msg4
    assert_eq!(session.messages().len(), 3);
    assert_eq!(session.messages()[0].content(), "msg2");
    assert_eq!(session.messages()[1].content(), "msg3");
    assert_eq!(session.messages()[2].content(), "msg4");

    // Add 3 more messages (bringing total to 6, over threshold again)
    for i in 5..8 {
        session
            .add_message(
                &store,
                Message::new("user".to_string(), format!("msg{}", i)),
            )
            .expect("Failed to add message");
    }

    // Should have 6 messages now
    assert_eq!(session.messages().len(), 6);

    // Trigger second compaction
    let compacted2 = session
        .maybe_compact(&store)
        .expect("Second compaction should succeed");

    assert!(compacted2, "Should have triggered compaction again");

    // Window should have slid: should keep last 3: msg5, msg6, msg7
    assert_eq!(
        session.messages().len(),
        3,
        "Window should slide to keep last 3 messages"
    );
    assert_eq!(session.messages()[0].content(), "msg5");
    assert_eq!(session.messages()[1].content(), "msg6");
    assert_eq!(session.messages()[2].content(), "msg7");
}

/// Test that LLM-based summarization is called with old messages.
/// Add 20 messages, configure to keep 5 recent, verify LLM called with 15 old messages.
#[test]
fn test_compact_summarize_calls_llm_with_old_messages() {
    use crate::session::{Message, SessionConfig};
    use std::sync::{Arc, Mutex};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-summarize-llm-call".to_string();
    let mut session = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    // Configure: threshold=10, keep_recent=5, strategy=Summarize
    session.set_config(SessionConfig {
        compaction_threshold: 10,
        compaction_strategy: crate::session::CompactionStrategy::Summarize,
        keep_recent: 5,
    });

    // Track what messages were passed to the summarizer
    let summarized_messages = Arc::new(Mutex::new(Vec::new()));
    let summarized_messages_clone = Arc::clone(&summarized_messages);

    // Create mock summarizer that records input
    let mock_summarizer = move |messages: &[Message]| -> std::io::Result<String> {
        let mut tracked = summarized_messages_clone.lock().unwrap();
        for msg in messages {
            tracked.push(msg.content().to_string());
        }
        Ok("Summary of conversation".to_string())
    };

    // Add 20 messages (well over threshold)
    for i in 0..20 {
        session
            .add_message(
                &store,
                Message::new("user".to_string(), format!("msg{}", i)),
            )
            .expect("Failed to add message");
    }

    // Verify we have 20 messages before compaction
    assert_eq!(session.messages().len(), 20);

    // Trigger compaction with mock summarizer
    session
        .compact_summarize_with(&store, mock_summarizer)
        .expect("Compaction should succeed");

    // Verify LLM was called with the 15 old messages (keeping 5 recent)
    let summarized = summarized_messages.lock().unwrap();
    assert_eq!(
        summarized.len(),
        15,
        "Should summarize 15 old messages (20 total - 5 recent)"
    );

    // Verify the correct messages were summarized (msg0 through msg14)
    for i in 0..15 {
        assert_eq!(
            summarized[i],
            format!("msg{}", i),
            "Message {} should be summarized",
            i
        );
    }
}

/// Test that summarize compaction replaces old messages with summary and preserves recent messages.
#[test]
fn test_compact_summarize_replaces_old_with_summary() {
    use crate::session::{Message, SessionConfig};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-summarize-replace".to_string();
    let mut session = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    // Configure: threshold=10, keep_recent=3, strategy=Summarize
    session.set_config(SessionConfig {
        compaction_threshold: 10,
        compaction_strategy: crate::session::CompactionStrategy::Summarize,
        keep_recent: 3,
    });

    // Mock summarizer that returns a fixed summary
    let mock_summarizer = |_messages: &[Message]| -> std::io::Result<String> {
        Ok("SUMMARY: Previous conversation context".to_string())
    };

    // Add 15 messages (over threshold)
    for i in 0..15 {
        session
            .add_message(
                &store,
                Message::new("user".to_string(), format!("msg{}", i)),
            )
            .expect("Failed to add message");
    }

    // Verify we have 15 messages before compaction
    assert_eq!(session.messages().len(), 15);

    // Trigger compaction with mock summarizer
    session
        .compact_summarize_with(&store, mock_summarizer)
        .expect("Compaction should succeed");

    // After compaction, should have: 1 summary message + 3 recent = 4 messages
    assert_eq!(
        session.messages().len(),
        4,
        "Should have 1 summary + 3 recent messages"
    );

    // Verify first message is the summary
    assert_eq!(session.messages()[0].role(), "system");
    assert_eq!(
        session.messages()[0].content(),
        "SUMMARY: Previous conversation context"
    );

    // Verify last 3 messages are the recent ones (msg12, msg13, msg14)
    assert_eq!(session.messages()[1].content(), "msg12");
    assert_eq!(session.messages()[2].content(), "msg13");
    assert_eq!(session.messages()[3].content(), "msg14");

    // Reload session from disk to verify persistence
    let loaded_session = store
        .load_session(&session_id)
        .expect("Failed to reload session");

    assert_eq!(loaded_session.messages().len(), 4);
    assert_eq!(loaded_session.messages()[0].role(), "system");
    assert_eq!(
        loaded_session.messages()[0].content(),
        "SUMMARY: Previous conversation context"
    );
    assert_eq!(loaded_session.messages()[1].content(), "msg12");
    assert_eq!(loaded_session.messages()[2].content(), "msg13");
    assert_eq!(loaded_session.messages()[3].content(), "msg14");
}

/// Test that rewrite_jsonl uses atomic writes via temp file + rename pattern.
///
/// This test verifies the crash-safety of JSONL rewriting by:
/// 1. Creating a session with messages
/// 2. Triggering compaction (which calls rewrite_jsonl)
/// 3. Verifying final file is correct after rename
/// 4. Verifying no temp files remain after successful write
///
/// The atomic pattern prevents corruption: if the process crashes during write,
/// the original file remains intact (temp file exists but original is untouched).
#[test]
fn test_rewrite_jsonl_uses_atomic_writes() {
    use crate::session::Message;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-atomic-write".to_string();
    let mut session = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    // Add messages to trigger compaction (need to exceed threshold of 100)
    for i in 0..105 {
        session
            .add_message(
                &store,
                Message::new("user".to_string(), format!("message {}", i)),
            )
            .expect("Failed to add message");
    }

    // Get the final session file path
    let jsonl_path = temp_dir.path().join(format!("{}.jsonl", session_id));
    let dir_path = jsonl_path.parent().expect("Should have parent dir");

    // Track files that exist during the write operation
    // This is a challenge because the write happens synchronously
    // Instead, we'll verify the implementation by checking:
    // 1. That rewrite_jsonl doesn't corrupt on partial writes
    // 2. That temp files are cleaned up

    // Store original content before compaction
    let original_content = fs::read_to_string(&jsonl_path).expect("Failed to read original file");
    let original_lines: Vec<&str> = original_content.lines().collect();
    let original_line_count = original_lines.len();

    // Trigger compaction (which calls rewrite_jsonl internally)
    let compacted = session
        .maybe_compact(&store)
        .expect("Failed to compact session");
    assert!(compacted, "Compaction should have been triggered");

    // After compaction, verify:
    // 1. Final file exists and is valid
    assert!(jsonl_path.exists(), "Final JSONL file should exist");

    let content = fs::read_to_string(&jsonl_path).expect("Failed to read final file");
    let lines: Vec<&str> = content.lines().collect();

    // Should have metadata + kept messages after compaction
    // Default sliding window keeps last 10 messages
    assert!(
        lines.len() >= 11,
        "Should have at least 1 metadata line + 10 message lines, got {}",
        lines.len()
    );

    // Verify we actually reduced the file size (compaction happened)
    assert!(
        lines.len() < original_line_count,
        "File should be smaller after compaction: before={}, after={}",
        original_line_count,
        lines.len()
    );

    // 2. No temp files should remain after successful write
    let temp_files: Vec<_> = fs::read_dir(dir_path)
        .expect("Failed to read directory")
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            // Check for any tempfile patterns (tempfile crate uses various patterns)
            name_str.starts_with(".tmp") || name_str.contains(".tmp") || name_str.starts_with("tmp")
        })
        .collect();

    assert_eq!(
        temp_files.len(),
        0,
        "No temp files should remain after atomic write completes. Found: {:?}",
        temp_files.iter().map(|e| e.file_name()).collect::<Vec<_>>()
    );

    // 3. Verify metadata and compaction count
    let metadata: serde_json::Value =
        serde_json::from_str(lines[0]).expect("First line should be valid metadata");

    assert_eq!(metadata.get("type").and_then(|v| v.as_str()), Some("meta"));
    assert_eq!(
        metadata.get("compaction_count").and_then(|v| v.as_u64()),
        Some(1),
        "Compaction count should be 1 after first compaction"
    );

    // 4. Verify all message lines are valid JSON
    for (i, line) in lines.iter().enumerate().skip(1) {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|e| panic!("Line {} should be valid JSON: {}", i, e));
    }
}

/// Test that verifies atomic write pattern is actually implemented.
///
/// This is a more direct test that checks implementation details:
/// - File should be written to a temp location first
/// - Then atomically renamed to final location
///
/// This test will FAIL with current implementation (fs::write) and
/// PASS when tempfile + persist pattern is implemented.
#[test]
fn test_atomic_write_implementation() {
    use crate::session::Message;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session_id = "test-atomic-impl".to_string();
    let mut session = store
        .get_or_create(Some(session_id.clone()))
        .expect("Failed to create session");

    // Add many messages to exceed compaction threshold
    for i in 0..105 {
        session
            .add_message(
                &store,
                Message::new("user".to_string(), format!("message {}", i)),
            )
            .expect("Failed to add message");
    }

    let jsonl_path = temp_dir.path().join(format!("{}.jsonl", session_id));

    // Read original file metadata to check if it gets replaced atomically
    let original_metadata = fs::metadata(&jsonl_path).expect("Original file should exist");
    let original_inode = original_metadata.ino();

    // Small delay to ensure different timestamp
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Trigger compaction
    session
        .maybe_compact(&store)
        .expect("Compaction should succeed");

    // After atomic rename, the inode should be DIFFERENT
    // because rename() replaces the target file
    let new_metadata = fs::metadata(&jsonl_path).expect("New file should exist");
    let new_inode = new_metadata.ino();

    // This assertion will FAIL with fs::write (same inode)
    // and PASS with atomic rename (different inode)
    assert_ne!(
        original_inode, new_inode,
        "Atomic write should result in different inode (temp file renamed). \
         Current implementation uses fs::write which modifies in-place. \
         Expected: temp file created -> written -> renamed (new inode). \
         Got: same inode (non-atomic in-place write)"
    );
}
