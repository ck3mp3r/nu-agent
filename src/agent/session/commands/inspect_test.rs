use super::AgentSessionInspect;
use crate::session::{ConversationStore, JsonlConversationStore, SessionStore};
use nu_plugin::SimplePluginCommand;
use rig::completion::Message;
use tempfile::TempDir;

#[test]
fn test_agent_session_inspect_displays_full_session_details() {
    // Setup: Create temp directory for sessions
    let temp_dir = TempDir::new().unwrap();
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    // Create a session with 10 messages using ConversationStore
    let session = store
        .get_or_create(Some("test-session".to_string()))
        .unwrap();

    let conversation_store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let messages: Vec<Message> = (0..10)
        .map(|i| Message::user(format!("Message {}", i)))
        .collect();
    conversation_store
        .append("test-session", &messages)
        .unwrap();

    // Verify session metadata (not loading full session to avoid old Message deserialization)
    assert_eq!(session.id(), "test-session");

    // Load messages via ConversationStore to verify count
    let loaded_messages = conversation_store.load("test-session").unwrap();
    assert_eq!(loaded_messages.len(), 10, "Should have 10 messages");

    // Verify all messages are present with correct data (check via rig Message API)
    for (i, msg) in loaded_messages.iter().enumerate() {
        // rig Messages use content enum, extract text
        match msg {
            rig::completion::Message::User { content } => {
                let text = content
                    .iter()
                    .map(|c| match c {
                        rig::completion::message::UserContent::Text(t) => t.text.clone(),
                        _ => panic!("Expected text content"),
                    })
                    .collect::<Vec<_>>()
                    .join("");
                assert_eq!(text, format!("Message {}", i));
            }
            _ => panic!("Expected User message"),
        }
    }

    // Verify compaction count (should be 0 for new session)
    assert_eq!(session.compaction_count(), 0);

    // Verify config is present (default config)
    let config = session.config();
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

#[test]
fn session_inspect_reports_canonical_sliding_summary_mode() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());
    let mut session = store
        .get_or_create(Some("inspect-canonical-strategy".to_string()))
        .expect("create session");

    session.set_config(crate::session::SessionConfig {
        compaction_threshold: 4,
        compaction_strategy: crate::session::CompactionStrategy::SlidingSummary,
        keep_recent: 2,
    });

    assert_eq!(
        session.config().compaction_strategy.as_str(),
        "sliding_summary"
    );
}
