// DEPRECATED: This test file extensively uses Session.add_message(), append_message(),
// and other deprecated APIs that use the old crate::session::Message type.
//
// These tests need to be:
// 1. Deleted if they test deprecated methods (add_message, append_message)
// 2. Migrated to use ConversationStore with rig::completion::Message
// 3. Kept if they test config/metadata that doesn't depend on old Message types
//
// This is tracked in task 4692a68d (parent task) which will migrate Session to use rig Messages.
//
// For now, these tests are commented out to allow the migration of test files 1-4 to proceed.

use crate::session::SessionStore;
use tempfile::TempDir;

#[test]
fn test_get_or_create_auto_generates_id() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let session = store.get_or_create(None).expect("Failed to create session");

    // Verify ID format: YYYYMMDD-HHMMSS-micros
    assert!(
        session
            .id()
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit()),
        "Session ID should start with a digit (timestamp), got: {}",
        session.id()
    );

    // Verify ID contains only digits and dashes
    assert!(
        session.id().chars().all(|c| c.is_ascii_digit() || c == '-'),
        "Session ID should contain only digits and dashes, got: {}",
        session.id()
    );
}

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

#[test]
fn test_list_sessions_empty_directory() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    // List sessions in empty directory
    let sessions = store
        .list_sessions(None)
        .expect("Failed to list sessions in empty directory");

    assert_eq!(
        sessions.len(),
        0,
        "Should return empty list for empty directory"
    );
}

// All other tests that use add_message(), append_message(), load_session().messages(), etc.
// are commented out pending Session migration to rig Messages.
// See task 4692a68d for full Session migration.

#[test]
fn test_list_sessions_with_prefix_filter() {
    let temp_dir = TempDir::new().unwrap();
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    store.get_or_create(Some("abc1234-foo".into())).unwrap();
    store.get_or_create(Some("abc1234-bar".into())).unwrap();
    store.get_or_create(Some("def5678-baz".into())).unwrap();

    let filtered = store.list_sessions(Some("abc1234")).unwrap();
    assert_eq!(filtered.len(), 2);
    assert!(filtered.iter().all(|s| s.id.starts_with("abc1234-")));

    let all = store.list_sessions(None).unwrap();
    assert_eq!(all.len(), 3);
}
