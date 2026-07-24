use super::AgentSessionClear;
use nu_agent_core::session::{FsSessionStore, SessionStore, SessionStoreImpl};
use nu_agent_core::types::Message;
use nu_plugin::SimplePluginCommand;
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
async fn test_agent_session_clear_deletes_existing_session() {
    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(SessionStoreImpl::Fs(FsSessionStore::new(
        temp_dir.path().to_path_buf(),
    )));

    // Create a session with a few messages
    let messages: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("Message {}", i)))
        .collect();
    store.create("test-session", &messages).await.unwrap();

    // Verify session file exists
    let session_path = temp_dir.path().join("test-session.jsonl");
    assert!(
        session_path.exists(),
        "Session file should exist before deletion"
    );

    // Execute command - delete the session
    let result = store.delete("test-session");

    // Verify result
    assert!(result.await.is_ok(), "Should successfully delete session");

    // Verify session file no longer exists
    assert!(
        !session_path.exists(),
        "Session file should be deleted after clear"
    );
}

#[tokio::test]
async fn test_agent_session_clear_is_idempotent_for_nonexistent_session() {
    // Setup: Create temp directory with no sessions
    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(SessionStoreImpl::Fs(FsSessionStore::new(
        temp_dir.path().to_path_buf(),
    )));

    // Execute command - try to delete non-existent session
    let result = store.delete("nonexistent");

    // Deletion of non-existent session is a no-op success
    assert!(
        result.await.is_ok(),
        "Should succeed (no-op) for nonexistent session"
    );
}

#[test]
fn test_agent_session_clear_command_signature() {
    let command = AgentSessionClear::new();

    // Verify command name
    assert_eq!(SimplePluginCommand::name(&command), "agent session clear");

    // Verify signature
    let sig = SimplePluginCommand::signature(&command);
    assert_eq!(sig.name, "agent session clear");

    // Should have one required positional parameter: session_id
    assert_eq!(sig.required_positional.len(), 1);
    assert_eq!(sig.required_positional[0].name, "id");
}

#[tokio::test]
async fn test_delete_session_removes_only_target_file() {
    // Setup: Create multiple sessions
    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(SessionStoreImpl::Fs(FsSessionStore::new(
        temp_dir.path().to_path_buf(),
    )));

    // Create three sessions (each with a minimal message so the file exists)
    store
        .create("session-1", &[Message::user("hello")])
        .await
        .unwrap();
    store
        .create("session-2", &[Message::user("hello")])
        .await
        .unwrap();
    store
        .create("session-3", &[Message::user("hello")])
        .await
        .unwrap();

    // Verify all three session files exist
    let path1 = temp_dir.path().join("session-1.jsonl");
    let path2 = temp_dir.path().join("session-2.jsonl");
    let path3 = temp_dir.path().join("session-3.jsonl");

    assert!(path1.exists());
    assert!(path2.exists());
    assert!(path3.exists());

    // Delete only session-2
    let result = store.delete("session-2");
    assert!(result.await.is_ok());

    // Verify only session-2 was deleted
    assert!(path1.exists(), "session-1 should still exist");
    assert!(!path2.exists(), "session-2 should be deleted");
    assert!(path3.exists(), "session-3 should still exist");
}
