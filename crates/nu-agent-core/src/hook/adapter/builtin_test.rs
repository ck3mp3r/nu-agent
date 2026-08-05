use super::*;
use crate::types::ToolDefinition;

/// Compile-time check that BuiltinToolAdapter implements Send + Sync.
///
/// This test ensures that our adapter can be safely shared across threads.
#[test]
fn builtin_tool_adapter_is_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    assert_send::<BuiltinToolAdapter>();
    assert_sync::<BuiltinToolAdapter>();
}

/// Compile-time check that DynamicTool is Send + Sync.
///
/// This ensures that DynamicTool instances can be registered with rig's ToolServer.
#[test]
fn dynamic_tool_is_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    assert_send::<DynamicTool>();
    assert_sync::<DynamicTool>();
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

    let adapter = BuiltinToolAdapter::new(tool_def, cwd.clone(), 20_000);

    assert_eq!(adapter.tool_def.name, "test_tool");
}

#[test]
fn adapter_returns_correct_description_and_parameters() {
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

    let adapter = BuiltinToolAdapter::new(tool_def.clone(), cwd.clone(), 20_000);

    assert_eq!(adapter.tool_def.name, "read");
    assert_eq!(adapter.tool_def.description, "Read a file");
    assert_eq!(adapter.tool_def.parameters, tool_def.parameters);
}

#[tokio::test]
async fn adapter_calls_skill_tool() {
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

    let temp_dir = std::env::temp_dir();
    let cwd = temp_dir.join("nu-agent-test-builtin-adapter");
    std::fs::create_dir_all(&cwd).unwrap();

    let adapter = BuiltinToolAdapter::new(tool_def, cwd.clone(), 20_000);

    let skill_dir = cwd.join(".agents").join("skills").join("test_skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    let skill_file = skill_dir.join("SKILL.md");
    std::fs::write(&skill_file, "# Test Skill\n\nThis is a test skill.").unwrap();

    // Convert to DynamicTool and execute via ToolSet
    let dynamic_tool = adapter.into_dynamic_tool();
    let mut toolset = rig::tool::ToolSet::default();
    toolset.add_dynamic_tool(dynamic_tool);

    let args = serde_json::json!({
        "name": "test_skill"
    });

    let mut context = rig::tool::ToolContext::new();
    let result = toolset
        .execute("skill", &args.to_string(), &mut context)
        .await;

    std::fs::remove_dir_all(&cwd).ok();

    assert!(result.is_success(), "Expected success, got: {:?}", result);
    let result_str = result.output().render();
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

#[tokio::test]
async fn adapter_truncates_large_output() {
    use crate::tools::limits::MAX_TOOL_OUTPUT_BYTES;

    let temp_dir = std::env::temp_dir();
    let cwd = temp_dir.join("nu-agent-test-builtin-adapter-truncate");
    std::fs::create_dir_all(&cwd).unwrap();

    let skill_dir = cwd.join(".agents").join("skills").join("big_skill");
    std::fs::create_dir_all(&skill_dir).unwrap();

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
    let adapter = BuiltinToolAdapter::new(tool_def, cwd.clone(), MAX_TOOL_OUTPUT_BYTES);

    // Convert to DynamicTool and execute via ToolSet
    let dynamic_tool = adapter.into_dynamic_tool();
    let mut toolset = rig::tool::ToolSet::default();
    toolset.add_dynamic_tool(dynamic_tool);

    let args = serde_json::json!({ "name": "big_skill" });
    let mut context = rig::tool::ToolContext::new();
    let result = toolset
        .execute("skill", &args.to_string(), &mut context)
        .await;

    std::fs::remove_dir_all(&cwd).ok();

    assert!(result.is_success(), "Expected success, got: {:?}", result);
    let result_str = result.output().render();

    assert!(
        result_str.contains("[output truncated:"),
        "large builtin output must be truncated; got {} bytes, no marker",
        result_str.len()
    );
}
