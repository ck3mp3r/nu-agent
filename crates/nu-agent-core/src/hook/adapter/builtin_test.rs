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

    let adapter = BuiltinToolAdapter::new(tool_def, cwd.clone(), 20_000);

    assert_eq!(adapter.name(), "test_tool");
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

    assert_eq!(adapter.name(), "read");
    assert_eq!(adapter.description(), "Read a file");
    assert_eq!(adapter.parameters(), tool_def.parameters);
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

    let adapter = BuiltinToolAdapter::new(tool_def, cwd.clone(), 20_000);

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
    let adapter = BuiltinToolAdapter::new(tool_def, cwd.clone(), MAX_TOOL_OUTPUT_BYTES);

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
