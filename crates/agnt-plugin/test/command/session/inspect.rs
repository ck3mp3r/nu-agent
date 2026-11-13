use super::inspect::{AgentSessionInspect, CwdInterface};
use agnt_runtime::config::CompactionConfig;
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
    let temp_dir = TempDir::new()?;
    let rig = rig_at(&temp_dir);
    let prefix = dir_prefix(temp_dir.path());
    let full_id = format!("{prefix}-test-session");
    let messages: Vec<Message> = (0..10)
        .map(|i| Message::user(format!("Message {}", i)))
        .collect();
    seed(&rig, &full_id, messages).await?;

    // -- Exec
    let command = AgentSessionInspect;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("test-session");
    let result = command
        .run_inner(&engine, &call, &rig, &CompactionConfig::default())
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
    let temp_dir = TempDir::new()?;
    let rig = rig_at(&temp_dir);

    // -- Exec
    let command = AgentSessionInspect;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("nonexistent");
    let result = command
        .run_inner(&engine, &call, &rig, &CompactionConfig::default())
        .await;

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
    let temp_dir = TempDir::new()?;
    let rig = rig_at(&temp_dir);
    let prefix = dir_prefix(temp_dir.path());
    let full_id = format!("{prefix}-my-session");
    seed(&rig, &full_id, vec![Message::user("hello")]).await?;

    // -- Exec
    let command = AgentSessionInspect;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("my-session");
    let result = command
        .run_inner(&engine, &call, &rig, &CompactionConfig::default())
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
async fn inspect_ignores_sessions_of_another_cwd() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new()?;
    let rig = rig_at(&temp_dir);

    // A session of another cwd must not be reachable from this cwd.
    let foreign_id = format!(
        "{}-old-session",
        dir_prefix(std::path::Path::new("/other/dir"))
    );
    seed(&rig, &foreign_id, vec![Message::user("hello")]).await?;

    // -- Exec
    let command = AgentSessionInspect;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("old-session");
    let result = command
        .run_inner(&engine, &call, &rig, &CompactionConfig::default())
        .await;

    // -- Check
    assert!(
        result.is_err(),
        "a session of another cwd must not resolve in this cwd"
    );
    Ok(())
}

#[tokio::test]
async fn test_agent_session_inspect_shows_the_default_compaction_policy() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new()?;
    let rig = rig_at(&temp_dir);
    let prefix = dir_prefix(temp_dir.path());
    let full_id = format!("{prefix}-test-session");
    seed(&rig, &full_id, vec![Message::user("hello")]).await?;
    let command = AgentSessionInspect;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("test-session");
    let default = agnt_runtime::rig_memory::CompactionPolicy::default();

    // -- Exec
    let result = command
        .run_inner(&engine, &call, &rig, &CompactionConfig::default())
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(
        config_int(&result, "keep_recent"),
        Some(default.keep_recent as i64)
    );
    assert_eq!(
        config_int(&result, "keep_asked"),
        Some(default.keep_asked as i64)
    );
    assert_eq!(
        config_int(&result, "reserve_pct"),
        Some(agnt_runtime::config::defaults::DEFAULT_RESERVE_PCT as i64)
    );
    // The 32k budget is our deliberate pin (c5t 7eb115fe), not rig's default.
    assert_eq!(config_int(&result, "summary_tokens"), Some(32_000));
    assert_eq!(
        config_int(&result, "keep_tokens"),
        Some(default.clearing.keep_tokens as i64)
    );
    assert_eq!(
        config_int(&result, "keep_under"),
        Some(default.clearing.keep_under as i64)
    );
    assert_eq!(
        config_str(&result, "near_window"),
        Some("summarize".to_string()),
        "the default near-window behavior is rig-memory's: summarize"
    );
    assert_eq!(
        config_tracked(&result),
        vec![
            (
                "read".to_string(),
                "path".to_string(),
                "read-files".to_string()
            ),
            (
                "edit".to_string(),
                "path".to_string(),
                "modified-files".to_string()
            ),
        ],
        "the two built-in tracked rules are the rows the policy resolved"
    );
    Ok(())
}

#[tokio::test]
async fn test_agent_session_inspect_shows_the_configured_compaction_policy() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new()?;
    let rig = rig_at(&temp_dir);
    let prefix = dir_prefix(temp_dir.path());
    let full_id = format!("{prefix}-test-session");
    seed(&rig, &full_id, vec![Message::user("hello")]).await?;
    let command = AgentSessionInspect;
    let engine = MockCwd {
        dir: temp_dir.path().to_string_lossy().to_string(),
    };
    let call = make_call("test-session");
    let config = CompactionConfig {
        keep_recent: Some(5000),
        reserve_pct: Some(35),
        clearing: Some(agnt_runtime::rig_memory::ClearToolOutputs {
            keep_tokens: 9000,
            keep_under: 250,
        }),
        near_window: Some(agnt_runtime::rig_memory::NearWindow::Clear),
        tracked: Some(vec![agnt_runtime::rig_memory::TrackArgument::new(
            "grep", "pattern", "grepped",
        )]),
        ..CompactionConfig::default()
    };

    // -- Exec
    let result = command
        .run_inner(&engine, &call, &rig, &config)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(config_int(&result, "keep_recent"), Some(5000));
    assert_eq!(config_int(&result, "reserve_pct"), Some(35));
    assert_eq!(config_int(&result, "keep_tokens"), Some(9000));
    assert_eq!(config_int(&result, "keep_under"), Some(250));
    assert_eq!(
        config_str(&result, "near_window"),
        Some("clear".to_string()),
        "a `near_window = \"clear\"` block renders as `clear`"
    );
    assert_eq!(
        config_tracked(&result),
        vec![(
            "grep".to_string(),
            "pattern".to_string(),
            "grepped".to_string()
        )],
        "an explicit tracked list renders its own rules"
    );
    Ok(())
}

// -- Test Support

/// One integer field of an inspected session's `config` record.
fn config_int(result: &Value, field: &str) -> Option<i64> {
    let Value::Record { val, .. } = result else {
        return None;
    };
    let Value::Record { val, .. } = val.get("config")? else {
        return None;
    };
    val.get(field)?.as_int().ok()
}

/// One string field of an inspected session's `config` record.
fn config_str(result: &Value, field: &str) -> Option<String> {
    let Value::Record { val, .. } = result else {
        return None;
    };
    let Value::Record { val, .. } = val.get("config")? else {
        return None;
    };
    val.get(field)?.as_str().ok().map(str::to_string)
}

/// The `(tool, argument, set)` rows of the inspected session's `tracked` field.
fn config_tracked(result: &Value) -> Vec<(String, String, String)> {
    let Value::Record { val, .. } = result else {
        return Vec::new();
    };
    let Some(Value::Record { val, .. }) = val.get("config") else {
        return Vec::new();
    };
    let Some(Value::List { vals, .. }) = val.get("tracked") else {
        return Vec::new();
    };
    vals.iter()
        .filter_map(|row| {
            let Value::Record { val, .. } = row else {
                return None;
            };
            Some((
                val.get("tool")?.as_str().ok()?.to_string(),
                val.get("argument")?.as_str().ok()?.to_string(),
                val.get("set")?.as_str().ok()?.to_string(),
            ))
        })
        .collect()
}

/// A file-backed rig store rooted at `temp_dir`.
fn rig_at(temp_dir: &TempDir) -> RigStoreConfig {
    RigStoreConfig::new(StoreType::Sqlite).with_cache_dir(temp_dir.path())
}

/// Seed the rig-memory store of `session_id` with `messages`.
async fn seed(rig: &RigStoreConfig, session_id: &str, messages: Vec<Message>) -> Result<()> {
    let store = rig.open(session_id).await.map_err(|e| format!("{e:?}"))?;
    let mut conversation =
        agnt_runtime::rig_memory::load(&*store, session_id).map_err(|e| format!("{e:?}"))?;
    conversation.extend(messages);
    agnt_runtime::rig_memory::save(&*store, session_id, &mut conversation)
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}
