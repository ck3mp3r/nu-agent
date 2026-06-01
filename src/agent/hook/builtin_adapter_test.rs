use super::*;
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
    let tool_def = rig::completion::ToolDefinition {
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

    let adapter = BuiltinToolAdapter::new(tool_def, cwd, None, None, None);

    assert_eq!(adapter.name(), "test_tool");
}

#[test]
fn adapter_returns_correct_definition() {
    use rig::tool::ToolDyn;

    let tool_def = rig::completion::ToolDefinition {
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

    let adapter = BuiltinToolAdapter::new(tool_def.clone(), cwd, None, None, None);

    // Since definition() is async, we need to use a runtime
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime.block_on(adapter.definition("".to_string()));

    assert_eq!(result.name, "read");
    assert_eq!(result.description, "Read a file");
}

#[test]
fn adapter_calls_skill_tool() {
    use rig::tool::ToolDyn;

    let tool_def = rig::completion::ToolDefinition {
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

    let adapter = BuiltinToolAdapter::new(tool_def, cwd.clone(), None, None, None);

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
// The dispatch_builtin_fs_tool function is already tested elsewhere.

#[test]
fn adapter_stores_agent_name() {
    let tool_def = rig::completion::ToolDefinition {
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
        cwd,
        None,
        None,
        Some("my-agent".to_string()),
    );

    assert_eq!(adapter.agent_name.as_deref(), Some("my-agent"));
}

#[test]
fn adapter_agent_name_defaults_to_none() {
    let tool_def = rig::completion::ToolDefinition {
        name: "send_message".to_string(),
        description: "Send a message".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {}
        }),
    };
    let cwd = std::path::PathBuf::from("/tmp");

    let adapter = BuiltinToolAdapter::new(tool_def, cwd, None, None, None);

    assert_eq!(adapter.agent_name, None);
}

#[test]
fn spawn_agent_without_orchestrator_returns_descriptive_error() {
    use rig::tool::ToolDyn;

    let tool_def = rig::completion::ToolDefinition {
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
    let adapter = BuiltinToolAdapter::new(tool_def, cwd, None, None, None);

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
