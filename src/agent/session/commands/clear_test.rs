use super::AgentSessionClear;
use crate::session::{Message, SessionStore};
use nu_plugin::SimplePluginCommand;
use tempfile::TempDir;

#[test]
fn test_agent_session_clear_deletes_existing_session() {
    // RED PHASE: This test will fail because AgentSessionClear doesn't exist yet

    // Setup: Create temp directory for sessions
    let temp_dir = TempDir::new().unwrap();
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    // Create a session with a few messages
    let mut session = store
        .get_or_create(Some("test-session".to_string()))
        .unwrap();

    for i in 0..5 {
        session
            .add_message(
                &store,
                Message::new("user".to_string(), format!("Message {}", i)),
            )
            .unwrap();
    }

    // Verify session file exists
    let session_path = store.cache_dir().join("test-session.jsonl");
    assert!(
        session_path.exists(),
        "Session file should exist before deletion"
    );

    // Execute command - delete the session
    let command = AgentSessionClear::new(store.clone());
    let result = command.store.delete_session("test-session");

    // Verify result
    assert!(result.is_ok(), "Should successfully delete session");

    // Verify session file no longer exists
    assert!(
        !session_path.exists(),
        "Session file should be deleted after clear"
    );
}

#[test]
fn test_agent_session_clear_returns_error_for_nonexistent_session() {
    // Setup: Create temp directory for sessions (but no sessions)
    let temp_dir = TempDir::new().unwrap();
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    // Execute command - try to delete non-existent session
    let command = AgentSessionClear::new(store.clone());
    let result = command.store.delete_session("nonexistent");

    // Verify result - should be an error
    assert!(
        result.is_err(),
        "Should return error for nonexistent session"
    );

    // The error message should indicate file not found
    let err = result.unwrap_err();
    assert_eq!(
        err.kind(),
        std::io::ErrorKind::NotFound,
        "Error should be NotFound"
    );
}

#[test]
fn test_agent_session_clear_command_signature() {
    let temp_dir = TempDir::new().unwrap();
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());
    let command = AgentSessionClear::new(store);

    // Verify command name
    assert_eq!(SimplePluginCommand::name(&command), "agent session clear");

    // Verify signature
    let sig = SimplePluginCommand::signature(&command);
    assert_eq!(sig.name, "agent session clear");

    // Should have one required positional parameter: session_id
    assert_eq!(sig.required_positional.len(), 1);
    assert_eq!(sig.required_positional[0].name, "id");
}

#[test]
fn test_delete_session_removes_only_target_file() {
    // Setup: Create multiple sessions
    let temp_dir = TempDir::new().unwrap();
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    // Create three sessions
    store.get_or_create(Some("session-1".to_string())).unwrap();
    store.get_or_create(Some("session-2".to_string())).unwrap();
    store.get_or_create(Some("session-3".to_string())).unwrap();

    // Verify all three session files exist
    let path1 = store.cache_dir().join("session-1.jsonl");
    let path2 = store.cache_dir().join("session-2.jsonl");
    let path3 = store.cache_dir().join("session-3.jsonl");

    assert!(path1.exists());
    assert!(path2.exists());
    assert!(path3.exists());

    // Delete only session-2
    let result = store.delete_session("session-2");
    assert!(result.is_ok());

    // Verify only session-2 was deleted
    assert!(path1.exists(), "session-1 should still exist");
    assert!(!path2.exists(), "session-2 should be deleted");
    assert!(path3.exists(), "session-3 should still exist");
}
