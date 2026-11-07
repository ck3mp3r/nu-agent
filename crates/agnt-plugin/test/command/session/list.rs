use super::list::{AgentSessionList, CwdInterface};
use agnt_runtime::session::prefix::dir_prefix;
use agnt_runtime::session::{RigStoreConfig, StoreType};
use agnt_runtime::types::Message;
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
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let rig = rig_at(&temp_dir);
    let prefix = dir_prefix(temp_dir.path());
    let session1 = format!("{prefix}-session1");
    let session2 = format!("{prefix}-session2");
    let messages1: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("msg {}", i)))
        .collect();
    let messages2: Vec<Message> = (0..10)
        .map(|i| Message::user(format!("msg {}", i)))
        .collect();
    seed(&rig, &session1, messages1).await?;
    seed(&rig, &session2, messages2).await?;

    // -- Exec
    let command = AgentSessionList;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call();
    let result = command
        .run_inner(&engine, &call, &rig)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
    let Value::List { vals, .. } = &result else {
        panic!("Expected list result");
    };
    assert_eq!(vals.len(), 2, "Should have 2 sessions");
    let count_of = |id: &str| -> Option<i64> {
        vals.iter().find_map(|v| match v {
            Value::Record { val, .. } if val.get("id")?.as_str().ok()? == id => {
                val.get("messages")?.as_int().ok()
            }
            _ => None,
        })
    };
    assert_eq!(count_of("session1"), Some(5), "session1 message count");
    assert_eq!(count_of("session2"), Some(10), "session2 message count");
    Ok(())
}

#[tokio::test]
async fn test_agent_session_list_returns_empty_list_when_no_sessions() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let rig = rig_at(&temp_dir);

    // -- Exec
    let command = AgentSessionList;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call();
    let result = command
        .run_inner(&engine, &call, &rig)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
    let Value::List { vals, .. } = &result else {
        panic!("Expected list result");
    };
    assert_eq!(vals.len(), 0, "Should have 0 sessions");
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
async fn list_returns_only_prefixed_sessions() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let rig = rig_at(&temp_dir);
    let prefix = dir_prefix(temp_dir.path());

    // One session carries the cwd prefix; the other carries a foreign prefix.
    seed(
        &rig,
        &format!("{prefix}-new-session"),
        vec![Message::user("hello")],
    )
    .await?;
    seed(
        &rig,
        &format!(
            "{}-foreign-session",
            dir_prefix(std::path::Path::new("/other/dir"))
        ),
        vec![Message::user("hello")],
    )
    .await?;

    // -- Exec
    let command = AgentSessionList;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call();
    let result = command
        .run_inner(&engine, &call, &rig)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
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
        "expected prefixed session in list, got: {ids:?}"
    );
    assert!(
        !ids.iter().any(|id| id.contains("foreign-session")),
        "a session of another cwd must not be listed, got: {ids:?}"
    );
    Ok(())
}

// -- Test Support

/// A file-backed rig store rooted at `temp_dir`.
fn rig_at(temp_dir: &TempDir) -> RigStoreConfig {
    RigStoreConfig::new(StoreType::Sqlite).with_cache_dir(temp_dir.path())
}

/// Seed the rig-memory store of `session_id` with `messages`, then write its
/// state records so its `meta.json` listing exists.
async fn seed(rig: &RigStoreConfig, session_id: &str, messages: Vec<Message>) -> Result<()> {
    let store = rig.open(session_id).await.map_err(|e| format!("{e:?}"))?;
    let mut conversation =
        agnt_runtime::rig_memory::load(&*store, session_id).map_err(|e| format!("{e:?}"))?;
    conversation.extend(messages);
    agnt_runtime::rig_memory::save(&*store, session_id, &mut conversation)
        .map_err(|e| format!("{e:?}"))?;
    agnt_runtime::session::rig_state::write_state_records(
        std::sync::Arc::clone(&store),
        "test/model".to_string(),
        Some("hello".to_string()),
    )
    .await
    .map_err(|e| format!("{e:?}"))?;
    Ok(())
}
