use super::AgentSessionInspect;
use crate::session::{Message, SessionStore};
use nu_plugin::SimplePluginCommand;
use tempfile::TempDir;

#[test]
fn test_agent_session_inspect_displays_full_session_details() {
    // RED PHASE: This test will fail because AgentSessionInspect doesn't exist yet

    // Setup: Create temp directory for sessions
    let temp_dir = TempDir::new().unwrap();
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    // Create a session with 10 messages
    let mut session = store
        .get_or_create(Some("test-session".to_string()))
        .unwrap();

    for i in 0..10 {
        session
            .add_message(
                &store,
                Message::new("user".to_string(), format!("Message {}", i)),
            )
            .unwrap();
    }

    // Execute command - test the underlying load_session() directly
    let command = AgentSessionInspect::new(store.clone());
    let loaded_session = command.store.load_session("test-session").unwrap();

    // Verify result
    assert_eq!(loaded_session.id(), "test-session");
    assert_eq!(
        loaded_session.messages().len(),
        10,
        "Should have 10 messages"
    );

    // Verify all messages are present with correct data
    for (i, msg) in loaded_session.messages().iter().enumerate() {
        assert_eq!(msg.role(), "user");
        assert_eq!(msg.content(), format!("Message {}", i));
    }

    // Verify compaction count (should be 0 for new session)
    assert_eq!(loaded_session.compaction_count(), 0);

    // Verify config is present (default config)
    let config = loaded_session.config();
    assert_eq!(config.compaction_threshold, 100);
}

#[test]
fn test_agent_session_inspect_returns_error_for_nonexistent_session() {
    // Setup: Create temp directory for sessions (but no sessions)
    let temp_dir = TempDir::new().unwrap();
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    // Execute command
    let command = AgentSessionInspect::new(store.clone());

    // Attempt to load non-existent session
    let result = command.store.load_session("nonexistent");

    // Verify result - should be an error
    assert!(
        result.is_err(),
        "Should return error for nonexistent session"
    );
}

#[test]
fn test_agent_session_inspect_command_signature() {
    let temp_dir = TempDir::new().unwrap();
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());
    let command = AgentSessionInspect::new(store);

    // Verify command name
    assert_eq!(SimplePluginCommand::name(&command), "agent session inspect");

    // Verify signature
    let sig = SimplePluginCommand::signature(&command);
    assert_eq!(sig.name, "agent session inspect");

    // Should have one required positional parameter: session_id
    assert_eq!(sig.required_positional.len(), 1);
    assert_eq!(sig.required_positional[0].name, "id");
}
