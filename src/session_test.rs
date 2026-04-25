use crate::session::SessionStore;
use std::fs;
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
