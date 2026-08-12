use crate::types::ToolDefinition;

/// Compile-time check that DynamicTool is Send + Sync.
///
/// This ensures that DynamicTool instances can be registered with rig's ToolServer.
#[test]
fn dynamic_tool_is_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    assert_send::<rig::tool::DynamicTool>();
    assert_sync::<rig::tool::DynamicTool>();
}

#[tokio::test]
async fn dynamic_tool_calls_grep_tool() {
    let tool_def = ToolDefinition {
        name: "grep".to_string(),
        description: "Search file contents".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string" }
            },
            "required": ["pattern"]
        }),
    };

    let temp_dir = std::env::temp_dir();
    let cwd = temp_dir.join("nu-agent-test-builtin-adapter");
    std::fs::create_dir_all(&cwd).unwrap();

    let bus = crate::bus::Bus::new();
    let dynamic_tool = super::make_dynamic_tool::<super::super::grep::GrepTool>(
        tool_def,
        cwd.clone(),
        20_000,
        bus,
    );

    let target_file = cwd.join("target.txt");
    std::fs::write(&target_file, "needle line\nother line\n").unwrap();

    let mut toolset = rig::tool::ToolSet::default();
    toolset.add_dynamic_tool(dynamic_tool);

    let args = serde_json::json!({ "pattern": "needle" });
    let mut context = rig::tool::ToolContext::new();
    let result = toolset
        .execute("grep", &args.to_string(), &mut context)
        .await;

    std::fs::remove_dir_all(&cwd).ok();

    assert!(result.is_success(), "Expected success, got: {:?}", result);
    let result_str = result.output().render();
    let result_json: serde_json::Value = serde_json::from_str(&result_str).unwrap();

    assert_eq!(result_json["total"], 1);
    assert!(
        result_json["matches"][0]["content"]
            .as_str()
            .unwrap()
            .contains("needle")
    );
}

#[tokio::test]
async fn dynamic_tool_truncates_large_output() {
    use crate::tools::limits::MAX_TOOL_OUTPUT_BYTES;

    let temp_dir = std::env::temp_dir();
    let cwd = temp_dir.join("nu-agent-test-builtin-adapter-truncate");
    std::fs::create_dir_all(&cwd).unwrap();

    // Create a file with many long matching lines so grep output exceeds the limit.
    let long_line = format!("needle {}\n", "x".repeat(300));
    let big_content = long_line.repeat(MAX_TOOL_OUTPUT_BYTES + 1_000);
    let target_file = cwd.join("big.txt");
    std::fs::write(&target_file, &big_content).unwrap();

    let tool_def = ToolDefinition {
        name: "grep".to_string(),
        description: "Search file contents".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string" }
            },
            "required": ["pattern"]
        }),
    };
    let bus = crate::bus::Bus::new();
    let dynamic_tool = super::make_dynamic_tool::<super::super::grep::GrepTool>(
        tool_def,
        cwd.clone(),
        MAX_TOOL_OUTPUT_BYTES,
        bus,
    );

    let mut toolset = rig::tool::ToolSet::default();
    toolset.add_dynamic_tool(dynamic_tool);

    let args = serde_json::json!({ "pattern": "needle" });
    let mut context = rig::tool::ToolContext::new();
    let result = toolset
        .execute("grep", &args.to_string(), &mut context)
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
