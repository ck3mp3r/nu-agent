use super::*;

/// The real `edit` tool registered through `make_dynamic_tool` must surface a
/// no-op plan as a failed rig `ToolResult`, so the hook chain's
/// `raw_result.is_success()` is false and the TUI shows `ItemStatus::Failed`.
#[tokio::test]
async fn dynamic_tool_edit_noop_surfaces_as_failed_result() -> TestResult<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "hello world\n")?;
    let version = crate::tools::fs::core::version_token("hello world\n");
    let tool_def = ToolDefinition {
        annotations: crate::types::ToolAnnotations::default(),
        name: ToolName::new("edit")?,
        description: "Edit a file".to_string(),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    };
    let dynamic_tool = super::super::make_dynamic_tool::<super::super::super::edit::EditTool>(
        tool_def,
        dir.path().to_path_buf(),
        20_000,
        crate::bus::Bus::default(),
    );
    let mut toolset = rig::tool::ToolSet::default();
    toolset.add_dynamic_tool(dynamic_tool);
    let args = serde_json::json!({
        "path": path.to_str().ok_or("temp path must be UTF-8")?,
        "expected_version": version,
        "operation": {
            "type": "search_replace",
            "search": "world",
            "replacement": "world"
        }
    });

    // -- Exec
    let mut context = rig::tool::ToolContext::new();
    let result = toolset
        .execute("edit", &args.to_string(), &mut context)
        .await;

    // -- Check
    assert!(
        !result.is_success(),
        "a no-op edit must not be a successful tool result"
    );
    let error = result.error().ok_or("no-op edit must surface as Err")?;
    assert_eq!(error.kind(), rig::tool::ToolErrorKind::InvalidArgs);
    assert!(
        error.message().contains("no change"),
        "error message must name the no-op, got: {}",
        error.message()
    );
    Ok(())
}
