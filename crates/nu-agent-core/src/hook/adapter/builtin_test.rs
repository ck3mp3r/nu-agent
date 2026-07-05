use super::*;
use crate::types::ToolDefinition;
use rig::tool::ToolDyn;

/// Compile-time check that BuiltinToolAdapter implements Send + Sync.
///
/// This test ensures that our adapter can be safely shared across threads,
/// which is required by rig's ToolDyn trait.
#[test]
fn builtin_tool_adapter_is_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    assert_send::<BuiltinToolAdapter>();
    assert_sync::<BuiltinToolAdapter>();
}

/// Compile-time check that ToolDyn trait object is Send + Sync.
///
/// This ensures that boxed trait objects can be registered with rig's ToolServer.
#[test]
fn tool_dyn_trait_object_is_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    assert_send::<Box<dyn ToolDyn>>();
    assert_sync::<Box<dyn ToolDyn>>();
}

#[test]
fn adapter_returns_correct_name() {
    let tool_def = ToolDefinition {
        name: "test_tool".to_string(),
        description: "A test tool".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "param": { "type": "string" }
            }
        }),
    };
    let cwd = std::path::PathBuf::from("/tmp");

    let adapter = BuiltinToolAdapter::new(tool_def, cwd.clone(), None, cwd.clone(), None, 20_000);

    assert_eq!(adapter.name(), "test_tool");
}

#[test]
fn adapter_returns_correct_definition() {
    use rig::tool::ToolDyn;

    let tool_def = ToolDefinition {
        name: "read".to_string(),
        description: "Read a file".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            },
            "required": ["path"]
        }),
    };
    let cwd = std::path::PathBuf::from("/tmp");

    let adapter = BuiltinToolAdapter::new(tool_def.clone(), cwd.clone(), None, cwd, None, 20_000);

    // Since definition() is async, we need to use a runtime
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime.block_on(adapter.definition("".to_string()));

    assert_eq!(result.name, "read");
    assert_eq!(result.description, "Read a file");
}

#[test]
fn adapter_calls_skill_tool() {
    use rig::tool::ToolDyn;

    let tool_def = ToolDefinition {
        name: "skill".to_string(),
        description: "Load skill content".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name"]
        }),
    };

    // Use a temp directory for testing
    let temp_dir = std::env::temp_dir();
    let cwd = temp_dir.join("nu-agent-test-builtin-adapter");
    std::fs::create_dir_all(&cwd).unwrap();

    let adapter = BuiltinToolAdapter::new(tool_def, cwd.clone(), None, cwd.clone(), None, 20_000);

    // Create a simple skill for testing
    let skill_dir = cwd.join(".agents").join("skills").join("test_skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    let skill_file = skill_dir.join("SKILL.md");
    std::fs::write(&skill_file, "# Test Skill\n\nThis is a test skill.").unwrap();

    let args = serde_json::json!({
        "name": "test_skill"
    });

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime.block_on(adapter.call(args.to_string()));

    // Clean up
    std::fs::remove_dir_all(&cwd).ok();

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let result_str = result.unwrap();
    let result_json: serde_json::Value = serde_json::from_str(&result_str).unwrap();

    assert_eq!(result_json["name"], "test_skill");
    assert!(
        result_json["content"]
            .as_str()
            .unwrap()
            .contains("Test Skill")
    );
}

// Note: Testing `read` tool would require actual files, which is more of an integration test.
// We verify the critical trait bounds (Send + Sync) and basic functionality here.
// The dispatch_fs_tool function is already tested elsewhere.

#[test]
fn adapter_stores_agent_name() {
    let tool_def = ToolDefinition {
        name: "send_message".to_string(),
        description: "Send a message".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "to": { "type": "string" },
                "message": { "type": "string" }
            },
            "required": ["to", "message"]
        }),
    };
    let cwd = std::path::PathBuf::from("/tmp");

    let adapter = BuiltinToolAdapter::new(
        tool_def,
        cwd.clone(),
        None,
        cwd,
        Some("my-agent".to_string()),
        20_000,
    );

    assert_eq!(adapter.agent_name.as_deref(), Some("my-agent"));
}

#[test]
fn adapter_agent_name_defaults_to_none() {
    let tool_def = ToolDefinition {
        name: "send_message".to_string(),
        description: "Send a message".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {}
        }),
    };
    let cwd = std::path::PathBuf::from("/tmp");

    let adapter = BuiltinToolAdapter::new(tool_def, cwd.clone(), None, cwd.clone(), None, 20_000);

    assert_eq!(adapter.agent_name, None);
}

#[test]
fn spawn_agent_without_orchestrator_returns_descriptive_error() {
    use rig::tool::ToolDyn;

    let tool_def = ToolDefinition {
        name: "spawn_agent".to_string(),
        description: "Spawn agent".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "agent": { "type": "string" }
            },
            "required": ["agent"]
        }),
    };
    let cwd = std::path::PathBuf::from("/tmp");

    // No orchestrator state — simulates a child agent calling spawn_agent
    let adapter = BuiltinToolAdapter::new(tool_def, cwd.clone(), None, cwd.clone(), None, 20_000);

    let args = serde_json::json!({ "agent": "coder" });
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime.block_on(adapter.call(args.to_string()));

    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("only available to orchestrator agents"),
        "Expected orchestrator error, got: {err_msg}"
    );
}

#[test]
fn adapter_truncates_large_output() {
    use crate::tools::limits::MAX_TOOL_OUTPUT_BYTES;
    use rig::tool::ToolDyn;

    // Write a skill file that is large enough to trigger truncation.
    // When the skill tool reads this file and serializes it as JSON, the
    // result will exceed MAX_TOOL_OUTPUT_BYTES, causing truncation.
    let temp_dir = std::env::temp_dir();
    let cwd = temp_dir.join("nu-agent-test-builtin-adapter-truncate");
    std::fs::create_dir_all(&cwd).unwrap();

    let skill_dir = cwd.join(".agents").join("skills").join("big_skill");
    std::fs::create_dir_all(&skill_dir).unwrap();

    // MAX_TOOL_OUTPUT_BYTES of 'x' to ensure the serialized JSON output
    // (which wraps content in a JSON string with extra fields) exceeds the limit.
    let big_content = "x".repeat(MAX_TOOL_OUTPUT_BYTES + 1_000);
    let skill_file = skill_dir.join("SKILL.md");
    std::fs::write(&skill_file, &big_content).unwrap();

    let tool_def = ToolDefinition {
        name: "skill".to_string(),
        description: "Load skill content".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name"]
        }),
    };
    let adapter = BuiltinToolAdapter::new(
        tool_def,
        cwd.clone(),
        None,
        cwd.clone(),
        None,
        MAX_TOOL_OUTPUT_BYTES,
    );

    let args = serde_json::json!({ "name": "big_skill" });
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime.block_on(adapter.call(args.to_string()));

    // Clean up
    std::fs::remove_dir_all(&cwd).ok();

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let result_str = result.unwrap();

    assert!(
        result_str.contains("[output truncated:"),
        "large builtin output must be truncated; got {} bytes, no marker",
        result_str.len()
    );
}

fn list_agents_tool_def() -> ToolDefinition {
    ToolDefinition {
        name: "list_agents".to_string(),
        description: "List agents".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {}
        }),
    }
}

#[tokio::test]
async fn list_agents_returns_pane_id_null_when_no_orchestrator() {
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    std::fs::File::create(dir.path().join("researcher-1.sock")).unwrap();

    let adapter = BuiltinToolAdapter::new(
        list_agents_tool_def(),
        dir.path().to_path_buf(),
        None, // no orchestrator
        dir.path().to_path_buf(),
        None,
        20_000,
    );

    let result = adapter.call("{}".to_string()).await.unwrap();
    let agents: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();

    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["name"], "researcher-1");
    assert_eq!(agents[0]["pane_id"], serde_json::Value::Null);
    assert_eq!(agents[0]["pane_alive"], false);
}

#[tokio::test]
async fn list_agents_returns_pane_id_when_orchestrator_tracks_it() {
    use crate::tools::handler::spawn_agent::OrchestratorState;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    std::fs::File::create(dir.path().join("researcher-1.sock")).unwrap();

    let mut state = OrchestratorState::new(dir.path().to_path_buf());
    state
        .agent_panes
        .insert("researcher-1".to_string(), "%99".to_string());

    let orchestrator = Arc::new(Mutex::new(state));

    let adapter = BuiltinToolAdapter::new(
        list_agents_tool_def(),
        dir.path().to_path_buf(),
        Some(orchestrator),
        dir.path().to_path_buf(),
        None,
        20_000,
    );

    let result = adapter.call("{}".to_string()).await.unwrap();
    let agents: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();

    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["name"], "researcher-1");
    assert_eq!(agents[0]["pane_id"], "%99");
    // pane_alive: false — RealTmuxRunner won't find this pane in tests
    assert_eq!(agents[0]["pane_alive"], false);
}

#[tokio::test]
async fn list_agents_cleans_up_dead_pane_entries() {
    use crate::tools::handler::spawn_agent::OrchestratorState;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    std::fs::File::create(dir.path().join("crashed.sock")).unwrap();

    let mut state = OrchestratorState::new(dir.path().to_path_buf());
    state
        .agent_panes
        .insert("crashed".to_string(), "%dead".to_string());

    let orchestrator = Arc::new(Mutex::new(state));

    let adapter = BuiltinToolAdapter::new(
        list_agents_tool_def(),
        dir.path().to_path_buf(),
        Some(Arc::clone(&orchestrator)),
        dir.path().to_path_buf(),
        None,
        20_000,
    );

    // First call: crashed agent appears with pane_alive: false
    let result = adapter.call("{}".to_string()).await.unwrap();
    let agents: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
    assert_eq!(agents[0]["name"], "crashed");
    assert_eq!(agents[0]["pane_alive"], false);

    // After the call, agent_panes should no longer contain "crashed"
    let state_after = orchestrator.lock().unwrap();
    assert!(
        !state_after.agent_panes.contains_key("crashed"),
        "Dead pane entry should have been auto-cleaned"
    );
}

#[tokio::test]
async fn list_agents_does_not_clean_agents_without_pane_id() {
    use crate::tools::handler::spawn_agent::OrchestratorState;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    // Socket exists but NOT tracked in agent_panes
    std::fs::File::create(dir.path().join("self-started.sock")).unwrap();

    let state = OrchestratorState::new(dir.path().to_path_buf());
    // agent_panes is empty — agent started its own socket
    let orchestrator = Arc::new(Mutex::new(state));

    let adapter = BuiltinToolAdapter::new(
        list_agents_tool_def(),
        dir.path().to_path_buf(),
        Some(Arc::clone(&orchestrator)),
        dir.path().to_path_buf(),
        None,
        20_000,
    );

    let result = adapter.call("{}".to_string()).await.unwrap();
    let agents: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();

    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["name"], "self-started");
    assert_eq!(agents[0]["pane_id"], serde_json::Value::Null);
    assert_eq!(agents[0]["pane_alive"], false);

    // agent_panes should remain empty — no cleanup of untracked agents
    let state_after = orchestrator.lock().unwrap();
    assert!(
        state_after.agent_panes.is_empty(),
        "agent_panes should remain empty — untracked agents must not be cleaned"
    );
}
