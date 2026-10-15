use super::*;

// ================================================================
// Handler-error mapping: details → truncated model output
// ================================================================

/// A builtin handler that always fails carrying a details payload.
struct FailingDetailsTool;

impl super::super::BuiltinTool for FailingDetailsTool {
    const NAME: &'static str = "fail_details";

    async fn execute(
        _args: &serde_json::Value,
        _cwd: &Path,
        _bus: &crate::bus::Bus,
    ) -> core::result::Result<serde_json::Value, super::super::super::ToolHandlerError> {
        Err(
            super::super::super::ToolHandlerError::runtime("boom").with_details(
                serde_json::json!({
                    "stdout": "x".repeat(4000),
                }),
            ),
        )
    }
}

/// A builtin handler that always fails without details.
struct FailingPlainTool;

impl super::super::BuiltinTool for FailingPlainTool {
    const NAME: &'static str = "fail_plain";

    async fn execute(
        _args: &serde_json::Value,
        _cwd: &Path,
        _bus: &crate::bus::Bus,
    ) -> core::result::Result<serde_json::Value, super::super::super::ToolHandlerError> {
        Err(super::super::super::ToolHandlerError::runtime("boom"))
    }
}

/// A builtin handler error maps to a rig `ToolExecutionError`. A
/// details-carrying error embeds the truncated details in the message and
/// attaches them as model output; a details-less error keeps the message only.
#[tokio::test]
async fn dynamic_tool_error_details_table() -> TestResult<()> {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        tool_name: &'static str,
        expected_message_prefix: &'static str,
        expects_details: bool,
    }

    let cases = vec![
        Case {
            name: "details are truncated and attached as model output",
            tool_name: "fail_details",
            expected_message_prefix: "boom: ",
            expects_details: true,
        },
        Case {
            name: "no details keeps the message only",
            tool_name: "fail_plain",
            expected_message_prefix: "boom: no details",
            expects_details: false,
        },
    ];

    let mut toolset = rig::tool::ToolSet::default();
    add_failing_tool::<FailingDetailsTool>(&mut toolset, "fail_details")?;
    add_failing_tool::<FailingPlainTool>(&mut toolset, "fail_plain")?;

    // -- Exec & Check
    for case in cases {
        let mut context = rig::tool::ToolContext::new();
        let result = toolset
            .execute(
                case.tool_name,
                &serde_json::json!({}).to_string(),
                &mut context,
            )
            .await;

        let error = result.error().ok_or("handler error must surface as Err")?;
        assert_eq!(
            error.kind(),
            rig::tool::ToolErrorKind::Other,
            "case '{}': kind",
            case.name
        );
        let message = error.message();
        assert!(
            message.starts_with(case.expected_message_prefix),
            "case '{}': message must start with '{}', got: {message}",
            case.name,
            case.expected_message_prefix
        );
        if case.expects_details {
            assert!(
                message.contains("[output truncated:"),
                "case '{}': details embedded in the message must be truncated, got: {message}",
                case.name
            );
            assert!(
                !message.contains(&"x".repeat(2000)),
                "case '{}': message must not embed the full untruncated payload",
                case.name
            );
            let feedback = error
                .model_feedback()
                .ok_or("model output must be attached for details-carrying errors")?;
            let embedded = message
                .strip_prefix("boom: ")
                .ok_or("message must prefix the handler message")?;
            assert_eq!(
                feedback, embedded,
                "case '{}': model output must equal the truncated details embedded in the message",
                case.name
            );
        } else {
            assert!(
                !message.contains("[output truncated:"),
                "case '{}': a details-less error must not embed a truncation marker, got: {message}",
                case.name
            );
        }
    }
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

        impl super::super::BuiltinTool for $struct_name {
            const NAME: &'static str = $tool_name;

            async fn execute(
                _args: &serde_json::Value,
                _cwd: &Path,
                _bus: &crate::bus::Bus,
            ) -> core::result::Result<serde_json::Value, super::super::super::ToolHandlerError>
            {
                Err($error)
            }
        }
    };
}

failing_kind_tool!(
    FailingValidationTool,
    "fail_validation",
    super::super::super::ToolHandlerError::validation("boom")
);
failing_kind_tool!(
    FailingTimeoutTool,
    "fail_timeout",
    super::super::super::ToolHandlerError {
        kind: super::super::super::ToolErrorKind::Timeout,
        message: "boom".to_string(),
        details: None,
    }
);
failing_kind_tool!(
    FailingAuthorizationTool,
    "fail_authorization",
    super::super::super::ToolHandlerError {
        kind: super::super::super::ToolErrorKind::Authorization,
        message: "boom".to_string(),
        details: None,
    }
);
failing_kind_tool!(
    FailingTransportTool,
    "fail_transport",
    super::super::super::ToolHandlerError {
        kind: super::super::super::ToolErrorKind::Transport,
        message: "boom".to_string(),
        details: None,
    }
);
failing_kind_tool!(
    FailingRuntimeTool,
    "fail_runtime",
    super::super::super::ToolHandlerError::runtime("boom")
);
failing_kind_tool!(
    FailingUnknownTool,
    "fail_unknown",
    super::super::super::ToolHandlerError {
        kind: super::super::super::ToolErrorKind::Unknown,
        message: "boom".to_string(),
        details: None,
    }
);
failing_kind_tool!(
    FailingValidationDetailsTool,
    "fail_validation_details",
    super::super::super::ToolHandlerError::validation("boom")
        .with_details(serde_json::json!({ "stdout": "x".repeat(4000) }))
);

/// Every handler error kind maps to the matching rig error kind, on both the
/// details-less and the details-carrying path.
#[tokio::test]
async fn dynamic_tool_error_kind_mapping_table() -> TestResult<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, rig::tool::ToolErrorKind, bool)> = vec![
        (
            "fail_validation",
            rig::tool::ToolErrorKind::InvalidArgs,
            false,
        ),
        ("fail_timeout", rig::tool::ToolErrorKind::Timeout, false),
        (
            "fail_authorization",
            rig::tool::ToolErrorKind::PermissionDenied,
            false,
        ),
        ("fail_transport", rig::tool::ToolErrorKind::Network, false),
        ("fail_runtime", rig::tool::ToolErrorKind::Other, false),
        ("fail_unknown", rig::tool::ToolErrorKind::Other, false),
        (
            "fail_validation_details",
            rig::tool::ToolErrorKind::InvalidArgs,
            true,
        ),
    ];

    let mut toolset = rig::tool::ToolSet::default();
    add_failing_tool::<FailingValidationTool>(&mut toolset, "fail_validation")?;
    add_failing_tool::<FailingTimeoutTool>(&mut toolset, "fail_timeout")?;
    add_failing_tool::<FailingAuthorizationTool>(&mut toolset, "fail_authorization")?;
    add_failing_tool::<FailingTransportTool>(&mut toolset, "fail_transport")?;
    add_failing_tool::<FailingRuntimeTool>(&mut toolset, "fail_runtime")?;
    add_failing_tool::<FailingUnknownTool>(&mut toolset, "fail_unknown")?;
    add_failing_tool::<FailingValidationDetailsTool>(&mut toolset, "fail_validation_details")?;

    // -- Exec & Check
    for (tool_name, expected_kind, expects_details) in cases {
        let mut context = rig::tool::ToolContext::new();
        let result = toolset
            .execute(tool_name, &serde_json::json!({}).to_string(), &mut context)
            .await;

        let error = result.error().ok_or("handler error must surface as Err")?;
        assert_eq!(
            error.kind(),
            expected_kind,
            "handler error kind for {tool_name} must map to the matching rig kind"
        );
        let message = error.message();
        assert!(
            message.starts_with("boom"),
            "message must keep the handler message, got: {message}"
        );
        if expects_details {
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
        }
    }
    Ok(())
}
