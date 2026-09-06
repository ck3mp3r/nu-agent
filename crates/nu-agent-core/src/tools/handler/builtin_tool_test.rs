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
    assert_eq!(error.kind(), rig::tool::ToolErrorKind::Other);
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

/// A builtin handler error without details maps the handler kind to the
/// rig kind (Runtime → Other) with the message only, no model-output
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
    assert_eq!(error.kind(), rig::tool::ToolErrorKind::Other);
    assert_eq!(
        error.message(),
        "boom: no details",
        "no-details mapping must stay unchanged"
    );
    Ok(())
}

// ================================================================
// Handler-error kind mapping: ToolErrorKind → rig ToolErrorKind
// ================================================================

/// Define a builtin handler that always fails with the given
/// `ToolHandlerError` expression.
macro_rules! failing_kind_tool {
    ($struct_name:ident, $tool_name:literal, $error:expr) => {
        struct $struct_name;

        impl super::BuiltinTool for $struct_name {
            const NAME: &'static str = $tool_name;

            async fn execute(
                _args: &serde_json::Value,
                _cwd: &Path,
                _bus: &crate::bus::Bus,
            ) -> core::result::Result<serde_json::Value, super::super::ToolHandlerError> {
                Err($error)
            }
        }
    };
}

failing_kind_tool!(
    FailingValidationTool,
    "fail_validation",
    super::super::ToolHandlerError::validation("boom")
);
failing_kind_tool!(
    FailingTimeoutTool,
    "fail_timeout",
    super::super::ToolHandlerError {
        kind: super::super::ToolErrorKind::Timeout,
        message: "boom".to_string(),
        details: None,
    }
);
failing_kind_tool!(
    FailingAuthorizationTool,
    "fail_authorization",
    super::super::ToolHandlerError {
        kind: super::super::ToolErrorKind::Authorization,
        message: "boom".to_string(),
        details: None,
    }
);
failing_kind_tool!(
    FailingTransportTool,
    "fail_transport",
    super::super::ToolHandlerError {
        kind: super::super::ToolErrorKind::Transport,
        message: "boom".to_string(),
        details: None,
    }
);
failing_kind_tool!(
    FailingRuntimeTool,
    "fail_runtime",
    super::super::ToolHandlerError::runtime("boom")
);
failing_kind_tool!(
    FailingUnknownTool,
    "fail_unknown",
    super::super::ToolHandlerError {
        kind: super::super::ToolErrorKind::Unknown,
        message: "boom".to_string(),
        details: None,
    }
);
failing_kind_tool!(
    FailingValidationDetailsTool,
    "fail_validation_details",
    super::super::ToolHandlerError::validation("boom")
        .with_details(serde_json::json!({ "stdout": "x".repeat(4000) }))
);

/// Every handler error kind maps to the matching rig error kind on the
/// details-less error path.
#[tokio::test]
async fn dynamic_tool_error_kind_maps_to_rig_kind_without_details() -> TestResult<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, rig::tool::ToolErrorKind)> = vec![
        ("fail_validation", rig::tool::ToolErrorKind::InvalidArgs),
        ("fail_timeout", rig::tool::ToolErrorKind::Timeout),
        (
            "fail_authorization",
            rig::tool::ToolErrorKind::PermissionDenied,
        ),
        ("fail_transport", rig::tool::ToolErrorKind::Network),
        ("fail_runtime", rig::tool::ToolErrorKind::Other),
        ("fail_unknown", rig::tool::ToolErrorKind::Other),
    ];
    let mut toolset = rig::tool::ToolSet::default();
    add_failing_tool::<FailingValidationTool>(&mut toolset, "fail_validation");
    add_failing_tool::<FailingTimeoutTool>(&mut toolset, "fail_timeout");
    add_failing_tool::<FailingAuthorizationTool>(&mut toolset, "fail_authorization");
    add_failing_tool::<FailingTransportTool>(&mut toolset, "fail_transport");
    add_failing_tool::<FailingRuntimeTool>(&mut toolset, "fail_runtime");
    add_failing_tool::<FailingUnknownTool>(&mut toolset, "fail_unknown");

    // -- Exec & Check
    for (tool_name, expected_kind) in &cases {
        let mut context = rig::tool::ToolContext::new();
        let result = toolset
            .execute(tool_name, &serde_json::json!({}).to_string(), &mut context)
            .await;

        let error = result.error().ok_or("handler error must surface as Err")?;
        assert_eq!(
            error.kind(),
            *expected_kind,
            "handler error kind for {tool_name} must map to the matching rig kind"
        );
        assert!(
            error.message().starts_with("boom"),
            "message must keep the handler message, got: {}",
            error.message()
        );
    }
    Ok(())
}

/// A details-carrying error keeps the kind mapping in the details branch:
/// the rig kind comes from the handler kind and the truncated details
/// stay embedded in the message and attached as model output.
#[tokio::test]
async fn dynamic_tool_error_kind_maps_to_rig_kind_with_details() -> TestResult<()> {
    // -- Setup & Fixtures
    let tool_def = ToolDefinition {
        name: "fail_validation_details".to_string(),
        description: "Always fails with details".to_string(),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    };
    let dynamic_tool = super::make_dynamic_tool::<FailingValidationDetailsTool>(
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
            "fail_validation_details",
            &serde_json::json!({}).to_string(),
            &mut context,
        )
        .await;

    // -- Check
    let error = result.error().ok_or("handler error must surface as Err")?;
    assert_eq!(error.kind(), rig::tool::ToolErrorKind::InvalidArgs);
    let message = error.message();
    assert!(
        message.starts_with("boom: {"),
        "details message must embed the truncated JSON, got: {message}"
    );
    assert!(
        message.contains("[output truncated:"),
        "details embedded in the message must be truncated, got: {message}"
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

// -- Test Support

fn add_failing_tool<T: super::BuiltinTool>(toolset: &mut rig::tool::ToolSet, tool_name: &str) {
    let tool_def = ToolDefinition {
        name: tool_name.to_string(),
        description: "Always fails".to_string(),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    };
    toolset.add_dynamic_tool(super::make_dynamic_tool::<T>(
        tool_def,
        std::env::temp_dir(),
        100,
        crate::bus::Bus::default(),
    ));
}
