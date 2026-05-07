use super::AgentSessionList;
use crate::session::{Message, SessionStore};
use nu_plugin::SimplePluginCommand;
use tempfile::TempDir;

#[test]
fn test_agent_session_list_returns_table_with_session_stats() {
    // Setup: Create temp directory for sessions
    let temp_dir = TempDir::new().unwrap();
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    // Create session 1 with 5 messages
    let mut session1 = store.get_or_create(Some("session1".to_string())).unwrap();
    for i in 0..5 {
        session1
            .add_message(
                &store,
                Message::new("user".to_string(), format!("msg {}", i)),
            )
            .unwrap();
    }

    // Create session 2 with 10 messages
    let mut session2 = store.get_or_create(Some("session2".to_string())).unwrap();
    for i in 0..10 {
        session2
            .add_message(
                &store,
                Message::new("user".to_string(), format!("msg {}", i)),
            )
            .unwrap();
    }

    // Execute command - test the underlying list_sessions() directly
    let command = AgentSessionList::new(store);

    // Actually, let's test just the session listing logic without the full plugin infrastructure
    // This is a more unit-test approach
    let sessions = command.store.list_sessions().unwrap();

    // Verify result
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
    assert_eq!(
        session1_info.compaction_count, 0,
        "Session1 should have 0 compactions"
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
    assert_eq!(
        session2_info.compaction_count, 0,
        "Session2 should have 0 compactions"
    );
}

#[test]
fn test_agent_session_list_returns_empty_list_when_no_sessions() {
    // Setup: Create temp directory for sessions (but don't create any)
    let temp_dir = TempDir::new().unwrap();
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    // Execute command
    let command = AgentSessionList::new(store);

    // Test the underlying list_sessions() directly
    let sessions = command.store.list_sessions().unwrap();

    // Verify result
    assert_eq!(sessions.len(), 0, "Should have 0 sessions");
}

#[test]
fn test_agent_session_list_command_signature() {
    let temp_dir = TempDir::new().unwrap();
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());
    let command = AgentSessionList::new(store);

    // Verify command name
    assert_eq!(SimplePluginCommand::name(&command), "agent session list");

    // Verify signature
    let sig = SimplePluginCommand::signature(&command);
    assert_eq!(sig.name, "agent session list");
}
