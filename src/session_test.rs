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
