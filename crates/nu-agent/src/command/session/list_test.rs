use super::AgentSessionList;
use nu_agent_core::session::{FsSessionStore, SessionStore, SessionStoreImpl};
use nu_agent_core::types::Message;
use nu_plugin::SimplePluginCommand;
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
async fn test_agent_session_list_returns_table_with_session_stats() {
    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(SessionStoreImpl::Fs(FsSessionStore::new(
        temp_dir.path().to_path_buf(),
    )));

    // Create session 1 with 5 messages
    let messages1: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("msg {}", i)))
        .collect();
    store.create("session1", &messages1).await.unwrap();

    // Create session 2 with 10 messages
    let messages2: Vec<Message> = (0..10)
        .map(|i| Message::user(format!("msg {}", i)))
        .collect();
    store.create("session2", &messages2).await.unwrap();

    // Test the underlying list() directly
    let sessions = store.list().await.unwrap();

    // Verify result (newest first)
    assert_eq!(sessions.len(), 2, "Should have 2 sessions");

    // Find session1 and verify its message count
    let session1_info = sessions
        .iter()
        .find(|s| s.id == "session1")
        .expect("Should find session1");

    assert_eq!(
        session1_info.message_count, 5,
        "Session1 should have 5 messages"
    );

    // Find session2 and verify its message count
    let session2_info = sessions
        .iter()
        .find(|s| s.id == "session2")
        .expect("Should find session2");

    assert_eq!(
        session2_info.message_count, 10,
        "Session2 should have 10 messages"
    );
}

#[tokio::test]
async fn test_agent_session_list_returns_empty_list_when_no_sessions() {
    // Setup: Create temp directory for sessions (but don't create any)
    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(SessionStoreImpl::Fs(FsSessionStore::new(
        temp_dir.path().to_path_buf(),
    )));

    // Test the underlying list() directly
    let sessions = store.list().await.unwrap();

    // Verify result
    assert_eq!(sessions.len(), 0, "Should have 0 sessions");
}

#[test]
fn test_agent_session_list_command_signature() {
    let command = AgentSessionList::new();

    // Verify command name
    assert_eq!(SimplePluginCommand::name(&command), "agent session list");

    // Verify signature
    let sig = SimplePluginCommand::signature(&command);
    assert_eq!(sig.name, "agent session list");
}
