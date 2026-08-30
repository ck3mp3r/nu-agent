use super::list::{AgentSessionList, CwdInterface};
use nu_agent_core::session::prefix::{dir_prefix, dir_prefix_legacy};
use nu_agent_core::session::{FsSessionStore, SessionStore, SessionStoreBackend};
use nu_agent_core::types::Message;
use nu_plugin::{EvaluatedCall, SimplePluginCommand};
use nu_protocol::{LabeledError, Span, Value};
use std::sync::Arc;
use tempfile::TempDir;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Minimal mock for CwdInterface that returns a fixed directory path.
struct MockCwd {
    dir: String,
}

impl CwdInterface for MockCwd {
    fn get_current_dir(&self) -> std::result::Result<String, LabeledError> {
        Ok(self.dir.clone())
    }
}

/// Helper: build a minimal EvaluatedCall with no positional arguments.
fn make_call() -> EvaluatedCall {
    let span = Span::test_data();
    EvaluatedCall {
        head: span,
        positional: vec![],
        named: vec![],
    }
}

#[tokio::test]
async fn test_agent_session_list_returns_table_with_session_stats() -> Result<()> {
    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(SessionStoreBackend::Fs(FsSessionStore::new(
        temp_dir.path().to_path_buf(),
    )));

    // Create session 1 with 5 messages
    let messages1: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("msg {}", i)))
        .collect();
    store
        .create("session1", &messages1)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // Create session 2 with 10 messages
    let messages2: Vec<Message> = (0..10)
        .map(|i| Message::user(format!("msg {}", i)))
        .collect();
    store
        .create("session2", &messages2)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // Test the underlying list() directly
    let sessions = store.list().await.map_err(|e| format!("{e:?}"))?;

    // Verify result (newest first)
    assert_eq!(sessions.len(), 2, "Should have 2 sessions");

    // Find session1 and verify its message count
    let session1_info = sessions
        .iter()
        .find(|s| s.id == "session1")
        .ok_or("should find session1")?;

    assert_eq!(
        session1_info.message_count, 5,
        "Session1 should have 5 messages"
    );

    // Find session2 and verify its message count
    let session2_info = sessions
        .iter()
        .find(|s| s.id == "session2")
        .ok_or("should find session2")?;

    assert_eq!(
        session2_info.message_count, 10,
        "Session2 should have 10 messages"
    );
    Ok(())
}

#[tokio::test]
async fn test_agent_session_list_returns_empty_list_when_no_sessions() -> Result<()> {
    // Setup: Create temp directory for sessions (but don't create any)
    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(SessionStoreBackend::Fs(FsSessionStore::new(
        temp_dir.path().to_path_buf(),
    )));

    // Test the underlying list() directly
    let sessions = store.list().await.map_err(|e| format!("{e:?}"))?;

    // Verify result
    assert_eq!(sessions.len(), 0, "Should have 0 sessions");
    Ok(())
}

#[test]
fn test_agent_session_list_command_signature() {
    let command = AgentSessionList;

    // Verify command name
    assert_eq!(SimplePluginCommand::name(&command), "agent session list");

    // Verify signature
    let sig = SimplePluginCommand::signature(&command);
    assert_eq!(sig.name, "agent session list");
}

#[tokio::test]
async fn list_returns_both_new_and_legacy_prefixed_sessions() -> Result<()> {
    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(SessionStoreBackend::Fs(FsSessionStore::new(
        temp_dir.path().to_path_buf(),
    )));

    let new_prefix = dir_prefix(temp_dir.path());
    let legacy_prefix = dir_prefix_legacy(temp_dir.path());

    // Create one session with the new 16-char prefix and one with the legacy 7-char prefix
    store
        .create(
            &format!("{new_prefix}-new-session"),
            &[Message::user("hello")],
        )
        .await
        .map_err(|e| format!("{e:?}"))?;
    store
        .create(
            &format!("{legacy_prefix}-legacy-session"),
            &[Message::user("hello")],
        )
        .await
        .map_err(|e| format!("{e:?}"))?;

    let command = AgentSessionList;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call();
    let result = command
        .run_inner(&engine, &call, &store)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // Extract the display IDs from the returned list
    let Value::List { vals, .. } = &result else {
        panic!("Expected list result");
    };
    let ids: Vec<String> = vals
        .iter()
        .filter_map(|v| match v {
            Value::Record { val, .. } => val
                .get("id")
                .and_then(|v| v.as_str().ok())
                .map(|s| s.to_owned()),
            _ => None,
        })
        .collect();

    assert!(
        ids.contains(&"new-session".to_string()),
        "expected new-prefixed session in list, got: {ids:?}"
    );
    assert!(
        ids.contains(&"legacy-session".to_string()),
        "expected legacy-prefixed session in list, got: {ids:?}"
    );
    Ok(())
}
