use super::inspect::{AgentSessionInspect, CwdInterface};
use agnt_core::session::prefix::{dir_prefix, dir_prefix_legacy};
use agnt_core::session::{RigStoreConfig, StoreType};
use agnt_core::types::Message;
use nu_plugin::{EvaluatedCall, SimplePluginCommand};
use nu_protocol::{LabeledError, Span, Value};
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
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let rig = rig_at(&temp_dir);
    let prefix = dir_prefix(temp_dir.path());
    let full_id = format!("{prefix}-test-session");
    let messages: Vec<Message> = (0..10)
        .map(|i| Message::user(format!("Message {}", i)))
        .collect();
    seed(&rig, &full_id, messages)?;

    // -- Exec
    let command = AgentSessionInspect;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("test-session");
    let result = command
        .run_inner(&engine, &call, &rig)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
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
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let rig = rig_at(&temp_dir);

    // -- Exec
    let command = AgentSessionInspect;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("nonexistent");
    let result = command.run_inner(&engine, &call, &rig).await;

    // -- Check
    assert!(
        result.is_err(),
        "inspecting a session that does not exist must report it"
    );
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
    // so the store is opened as "<prefix>-my-session" rather than the raw "my-session".

    // -- Setup & Fixtures
    let temp_dir = TempDir::new().expect("tempdir");
    let rig = rig_at(&temp_dir);
    let prefix = dir_prefix(temp_dir.path());
    let full_id = format!("{prefix}-my-session");
    seed(&rig, &full_id, vec![Message::user("hello")])?;

    // -- Exec
    let command = AgentSessionInspect;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("my-session");
    let result = command
        .run_inner(&engine, &call, &rig)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
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
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let rig = rig_at(&temp_dir);

    // Create a session with the legacy 7-char prefix (as if created before the
    // prefix length increase).
    let legacy_prefix = dir_prefix_legacy(temp_dir.path());
    let legacy_id = format!("{legacy_prefix}-old-session");
    seed(&rig, &legacy_id, vec![Message::user("hello")])?;

    // -- Exec
    let command = AgentSessionInspect;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("old-session");
    let result = command
        .run_inner(&engine, &call, &rig)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
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

// -- Test Support

/// A `DirStore` rig store rooted at `temp_dir`.
fn rig_at(temp_dir: &TempDir) -> RigStoreConfig {
    RigStoreConfig::new(StoreType::Jsonl).with_cache_dir(temp_dir.path())
}

/// Seed the rig-memory store of `session_id` with `messages`.
fn seed(rig: &RigStoreConfig, session_id: &str, messages: Vec<Message>) -> Result<()> {
    let store = rig.open(session_id).map_err(|e| format!("{e:?}"))?;
    let mut conversation =
        agnt_core::rig_memory::load(&*store, session_id).map_err(|e| format!("{e:?}"))?;
    conversation.extend(messages);
    agnt_core::rig_memory::save(&*store, session_id, &mut conversation)
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}
