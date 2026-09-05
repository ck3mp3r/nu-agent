use crate::types::ToolDefinition;
use std::path::Path;

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

    let bus = crate::bus::Bus::default();
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
    let bus = crate::bus::Bus::default();
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

// ================================================================
// Handler-error mapping: details → truncated model output
// ================================================================

type TestResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A builtin handler that always fails carrying a details payload.
struct FailingDetailsTool;

impl super::BuiltinTool for FailingDetailsTool {
    const NAME: &'static str = "fail_details";

    async fn execute(
        _args: &serde_json::Value,
        _cwd: &Path,
        _bus: &crate::bus::Bus,
    ) -> core::result::Result<serde_json::Value, super::super::ToolHandlerError> {
        Err(
            super::super::ToolHandlerError::runtime("boom").with_details(serde_json::json!({
                "stdout": "x".repeat(4000),
            })),
        )
    }
}

/// A builtin handler that always fails without details.
struct FailingPlainTool;

impl super::BuiltinTool for FailingPlainTool {
    const NAME: &'static str = "fail_plain";

    async fn execute(
        _args: &serde_json::Value,
        _cwd: &Path,
        _bus: &crate::bus::Bus,
    ) -> core::result::Result<serde_json::Value, super::super::ToolHandlerError> {
        Err(super::super::ToolHandlerError::runtime("boom"))
    }
}

/// A builtin handler error carrying details maps to a ToolExecutionError
/// whose message embeds only the truncated details and whose model output
/// is exactly the truncated details JSON.
#[tokio::test]
async fn dynamic_tool_error_with_details_truncates_and_sets_model_output() -> TestResult<()> {
    // -- Setup & Fixtures
    let tool_def = ToolDefinition {
        name: "fail_details".to_string(),
        description: "Always fails with details".to_string(),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    };
    let dynamic_tool = super::make_dynamic_tool::<FailingDetailsTool>(
        tool_def,
        std::env::temp_dir(),
        100,
        crate::bus::Bus::default(),
    );
    let mut toolset = rig::tool::ToolSet::default();
    toolset.add_dynamic_tool(dynamic_tool);

    // -- Exec
    let mut context = rig::tool::ToolContext::new();
    let result = toolset
        .execute(
            "fail_details",
            &serde_json::json!({}).to_string(),
            &mut context,
        )
        .await;

    // -- Check
    let error = result.error().ok_or("handler error must surface as Err")?;
    assert_eq!(error.kind(), rig::tool::ToolErrorKind::Provider);
    let message = error.message();
    assert!(
        message.contains("[output truncated:"),
        "details embedded in the message must be truncated, got: {message}"
    );
    assert!(
        !message.contains(&"x".repeat(2000)),
        "message must not embed the full untruncated payload"
    );
    let feedback = error
        .model_feedback()
        .ok_or("model output must be attached for details-carrying errors")?;
    let embedded = message
        .strip_prefix("boom: ")
        .ok_or("message must prefix the handler message")?;
    assert_eq!(
        feedback, embedded,
        "model output must equal the truncated details embedded in the message"
    );
    Ok(())
}

/// A builtin handler error without details keeps the current mapping:
/// ToolExecutionError::provider with the message only, no model-output
/// override.
#[tokio::test]
async fn dynamic_tool_error_without_details_keeps_message_only() -> TestResult<()> {
    // -- Setup & Fixtures
    let tool_def = ToolDefinition {
        name: "fail_plain".to_string(),
        description: "Always fails without details".to_string(),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    };
    let dynamic_tool = super::make_dynamic_tool::<FailingPlainTool>(
        tool_def,
        std::env::temp_dir(),
        100,
        crate::bus::Bus::default(),
    );
    let mut toolset = rig::tool::ToolSet::default();
    toolset.add_dynamic_tool(dynamic_tool);

    // -- Exec
    let mut context = rig::tool::ToolContext::new();
    let result = toolset
        .execute(
            "fail_plain",
            &serde_json::json!({}).to_string(),
            &mut context,
        )
        .await;

    // -- Check
    let error = result.error().ok_or("handler error must surface as Err")?;
    assert_eq!(error.kind(), rig::tool::ToolErrorKind::Provider);
    assert_eq!(
        error.message(),
        "boom: no details",
        "no-details mapping must stay unchanged"
    );
    Ok(())
}
