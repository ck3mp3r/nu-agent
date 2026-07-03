use super::inspect::{AgentSessionInspect, CwdInterface};
use nu_agent_core::session::prefix::dir_prefix;
use nu_agent_core::session::{ConversationStore, JsonlConversationStore, SessionStore};
use nu_agent_core::types::{Message, UserContent};
use nu_plugin::{EvaluatedCall, SimplePluginCommand};
use nu_protocol::{LabeledError, Span, Value};
use tempfile::TempDir;

/// Minimal mock for CwdInterface that returns a fixed directory path.
struct MockCwd {
    dir: String,
}

impl CwdInterface for MockCwd {
    fn get_current_dir(&self) -> Result<String, LabeledError> {
        Ok(self.dir.clone())
    }
}

/// Helper: build a minimal EvaluatedCall with a single positional string argument.
fn make_call(session_id: &str) -> EvaluatedCall {
    let span = Span::test_data();
    EvaluatedCall {
        head: span,
        positional: vec![Value::string(session_id, span)],
        named: vec![],
    }
}

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
        .append("test-session", &messages, None)
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
            Message::User { content } => {
                let text = content
                    .iter()
                    .map(|c| match c {
                        UserContent::Text(t) => t.text.clone(),
                        _ => panic!("Expected text content"),
                    })
                    .collect::<Vec<_>>()
                    .join("");
                assert_eq!(text, format!("Message {}", i));
            }
            _ => panic!("Expected User message"),
        }
    }

    // Verify config is present (default config)
    let config = session.compaction_config();
    assert_eq!(
        config.compaction_strategy,
        nu_agent_core::compaction::CompactionStrategy::SlidingSummary
    );
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

    session.set_compaction_config(nu_agent_core::compaction::CompactionParams {
        compaction_strategy: nu_agent_core::compaction::CompactionStrategy::SlidingSummary,
        keep_recent: 2,
        token_budget: None,
    });

    assert_eq!(
        session.compaction_config().compaction_strategy.as_str(),
        "sliding_summary"
    );
}

#[test]
fn run_prepends_cwd_prefix_to_session_id() {
    // Verify that run_inner() prepends the cwd-derived prefix to the user-supplied session ID,
    // so store.load_session receives "<prefix>-my-session" rather than the raw "my-session".

    let temp_dir = TempDir::new().expect("tempdir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    // Compute the prefix the same way dir_prefix() does
    let prefix = dir_prefix(temp_dir.path());

    // Seed the session with the full prefixed ID — this is what run_inner must look up
    store
        .get_or_create(Some(format!("{prefix}-my-session")))
        .expect("create session");

    let command = AgentSessionInspect::new(store);
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("my-session");

    // run_inner must prepend the prefix and succeed; without the fix it would pass "my-session"
    // directly to load_session, which would fail with a "session not found" error.
    let result = command.run_inner(&engine, &call);
    assert!(result.is_ok(), "run_inner failed: {:?}", result.err());

    let value = result.unwrap();
    let id = match &value {
        Value::Record { val, .. } => val
            .get("id")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_owned()),
        _ => None,
    };
    assert_eq!(
        id.as_deref(),
        Some(format!("{prefix}-my-session").as_str()),
        "returned id should equal the prefixed session id"
    );
}
