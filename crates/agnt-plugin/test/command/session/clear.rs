use super::clear::{AgentSessionClear, CwdInterface};
use agnt_core::session::prefix::dir_prefix_legacy;
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
async fn test_agent_session_clear_deletes_existing_session() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let rig = rig_at(&temp_dir);
    let prefix = agnt_core::session::prefix::dir_prefix(temp_dir.path());
    let full_id = format!("{prefix}-test-session");
    seed(&rig, &full_id, vec![Message::user("hello")])?;

    // -- Exec
    let command = AgentSessionClear;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("test-session");
    command
        .run_inner(&engine, &call, &rig)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert!(
        !rig.holds(&full_id).map_err(|e| format!("{e:?}"))?,
        "the session's rig store must be gone after clear"
    );
    Ok(())
}

#[tokio::test]
async fn test_agent_session_clear_reports_a_missing_session() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let rig = rig_at(&temp_dir);

    // -- Exec
    let command = AgentSessionClear;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("nonexistent");
    let result = command.run_inner(&engine, &call, &rig).await;

    // -- Check
    assert!(
        result.is_err(),
        "clearing a session that does not exist must report it"
    );
    Ok(())
}

#[test]
fn test_agent_session_clear_command_signature() {
    let command = AgentSessionClear;

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
async fn test_delete_session_removes_only_target_session() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let rig = rig_at(&temp_dir);
    let prefix = agnt_core::session::prefix::dir_prefix(temp_dir.path());
    let ids: Vec<String> = ["session-1", "session-2", "session-3"]
        .into_iter()
        .map(|name| format!("{prefix}-{name}"))
        .collect();
    for id in &ids {
        seed(&rig, id, vec![Message::user("hello")])?;
    }

    // -- Exec
    let command = AgentSessionClear;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("session-2");
    command
        .run_inner(&engine, &call, &rig)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert!(
        rig.holds(&ids[0]).map_err(|e| format!("{e:?}"))?,
        "session-1 should still exist"
    );
    assert!(
        !rig.holds(&ids[1]).map_err(|e| format!("{e:?}"))?,
        "session-2 should be deleted"
    );
    assert!(
        rig.holds(&ids[2]).map_err(|e| format!("{e:?}"))?,
        "session-3 should still exist"
    );
    Ok(())
}

#[tokio::test]
async fn clear_deletes_legacy_prefixed_session_via_fallback() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let rig = rig_at(&temp_dir);

    // Create a session with the legacy 7-char prefix (as if created before the
    // prefix length increase).
    let legacy_prefix = dir_prefix_legacy(temp_dir.path());
    let legacy_id = format!("{legacy_prefix}-old-session");
    seed(&rig, &legacy_id, vec![Message::user("hello")])?;

    // -- Exec
    let command = AgentSessionClear;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("old-session");
    command
        .run_inner(&engine, &call, &rig)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert!(
        !rig.holds(&legacy_id).map_err(|e| format!("{e:?}"))?,
        "legacy-prefixed session should be deleted after clear"
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
