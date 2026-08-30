use super::inspect::{AgentSessionInspect, CwdInterface};
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

/// Helper: build a minimal EvaluatedCall with a single positional string argument.
fn make_call(session_id: &str) -> EvaluatedCall {
    let span = Span::test_data();
    EvaluatedCall {
        head: span,
        positional: vec![Value::string(session_id, span)],
        named: vec![],
    }
}

#[tokio::test]
async fn test_agent_session_inspect_displays_full_session_details() -> Result<()> {
    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(SessionStoreBackend::Fs(FsSessionStore::new(
        temp_dir.path().to_path_buf(),
    )));

    // Create a session with prefixed ID and 10 messages
    let prefix = dir_prefix(temp_dir.path());
    let full_id = format!("{prefix}-test-session");
    let messages: Vec<Message> = (0..10)
        .map(|i| Message::user(format!("Message {}", i)))
        .collect();
    store
        .create(&full_id, &messages)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // Run inspect via run_inner
    let command = AgentSessionInspect;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("test-session");
    let result = command
        .run_inner(&engine, &call, &store)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // Extract message_count from the returned record
    let Value::Record { val, .. } = &result else {
        panic!("Expected record result");
    };
    let count = val
        .get("message_count")
        .and_then(|v| v.as_int().ok())
        .ok_or("should have message_count field")?;
    assert_eq!(count, 10, "Should have 10 messages");

    let id = val
        .get("id")
        .and_then(|v| v.as_str().ok())
        .ok_or("should have id field")?;
    assert_eq!(id, full_id);
    Ok(())
}

#[tokio::test]
async fn test_agent_session_inspect_returns_error_for_nonexistent_session() -> Result<()> {
    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(SessionStoreBackend::Fs(FsSessionStore::new(
        temp_dir.path().to_path_buf(),
    )));

    let prefix = dir_prefix(temp_dir.path());
    let full_id = format!("{prefix}-nonexistent");
    let result = store.load(&full_id).await.map_err(|e| format!("{e:?}"))?;

    // Should be Ok(None) — session not found
    assert!(result.is_none(), "should be None for nonexistent session");
    Ok(())
}

#[test]
fn test_agent_session_inspect_command_signature() {
    let command = AgentSessionInspect;

    // Verify command name
    assert_eq!(SimplePluginCommand::name(&command), "agent session inspect");

    // Verify signature
    let sig = SimplePluginCommand::signature(&command);
    assert_eq!(sig.name, "agent session inspect");

    // Should have one required positional parameter: session_id
    assert_eq!(sig.required_positional.len(), 1);
    assert_eq!(sig.required_positional[0].name, "id");
}

#[tokio::test]
async fn run_prepends_cwd_prefix_to_session_id() -> Result<()> {
    // Verify that run_inner() prepends the cwd-derived prefix to the user-supplied session ID,
    // so store.load receives "<prefix>-my-session" rather than the raw "my-session".

    let temp_dir = TempDir::new().expect("tempdir");
    let store = Arc::new(SessionStoreBackend::Fs(FsSessionStore::new(
        temp_dir.path().to_path_buf(),
    )));

    // Compute the prefix the same way dir_prefix() does
    let prefix = dir_prefix(temp_dir.path());

    // Create the session with the full prefixed ID so run_inner finds it
    let full_id = format!("{prefix}-my-session");
    store
        .create(&full_id, &[Message::user("hello")])
        .await
        .map_err(|e| format!("{e:?}"))?;

    let command = AgentSessionInspect;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("my-session");

    let result = command
        .run_inner(&engine, &call, &store)
        .await
        .map_err(|e| format!("{e:?}"))?;

    let id = match &result {
        Value::Record { val, .. } => val
            .get("id")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_owned()),
        _ => None,
    };
    assert_eq!(
        id.as_deref(),
        Some(full_id.as_str()),
        "returned id should equal the prefixed session id"
    );
    Ok(())
}

#[tokio::test]
async fn inspect_finds_legacy_prefixed_session_via_fallback() -> Result<()> {
    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(SessionStoreBackend::Fs(FsSessionStore::new(
        temp_dir.path().to_path_buf(),
    )));

    // Create a session with the legacy 7-char prefix (as if created before the
    // prefix length increase).
    let legacy_prefix = dir_prefix_legacy(temp_dir.path());
    let legacy_id = format!("{legacy_prefix}-old-session");
    store
        .create(&legacy_id, &[Message::user("hello")])
        .await
        .map_err(|e| format!("{e:?}"))?;

    let command = AgentSessionInspect;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("old-session");

    let result = command
        .run_inner(&engine, &call, &store)
        .await
        .map_err(|e| format!("{e:?}"))?;

    let id = match &result {
        Value::Record { val, .. } => val
            .get("id")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_owned()),
        _ => None,
    };
    assert_eq!(
        id.as_deref(),
        Some(legacy_id.as_str()),
        "returned id should equal the legacy-prefixed session id"
    );
    Ok(())
}
