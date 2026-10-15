//! `ToolError` → rig `ToolExecutionError` mapping tests.

use super::*;

use crate::tools::audit::AuditError;
use crate::tools::error::ToolError;
use nu_protocol::shell_error::generic::GenericError;
use nu_protocol::{ShellError, Span};
use std::time::Duration;

/// A timeout ToolError maps to rig's timeout kind and keeps the ToolError
/// Display output as the message.
#[test]
fn map_tool_error_timeout_returns_timeout_kind_with_display_message() -> TestResult<()> {
    // -- Setup & Fixtures
    let tool_error = ToolError::Timeout {
        tool_name: "grep".to_string(),
        timeout: Duration::from_secs(30),
    };
    let expected_message = tool_error.to_string();

    // -- Exec
    let mapped = map_tool_error(tool_error);

    // -- Check
    assert_eq!(mapped.kind(), rig::tool::ToolErrorKind::Timeout);
    assert_eq!(
        mapped.message(),
        "Tool 'grep' timed out after 30s",
        "timeout message must stay byte-identical to the Display output"
    );
    assert_eq!(mapped.message(), expected_message);
    Ok(())
}

/// An execution ToolError maps to rig's other kind and keeps the ToolError
/// Display output as the message.
#[test]
fn map_tool_error_execution_returns_other_kind_with_display_message() -> TestResult<()> {
    // -- Setup & Fixtures
    let shell_error: ShellError = GenericError::new("boom", "details", Span::unknown()).into();
    let tool_error = ToolError::Execution(Box::new(shell_error));
    let expected_message = tool_error.to_string();

    // -- Exec
    let mapped = map_tool_error(tool_error);

    // -- Check
    assert_eq!(mapped.kind(), rig::tool::ToolErrorKind::Other);
    assert_eq!(mapped.message(), expected_message);
    assert!(
        mapped.message().starts_with("Tool execution failed: "),
        "message must keep the ToolError Display output, got: {}",
        mapped.message()
    );
    Ok(())
}

/// An audit ToolError maps to rig's other kind and keeps the ToolError
/// Display output as the message.
#[test]
fn map_tool_error_audit_returns_other_kind_with_display_message() -> TestResult<()> {
    // -- Setup & Fixtures
    let tool_error = ToolError::Audit(AuditError::Write("disk full".to_string()));
    let expected_message = tool_error.to_string();

    // -- Exec
    let mapped = map_tool_error(tool_error);

    // -- Check
    assert_eq!(mapped.kind(), rig::tool::ToolErrorKind::Other);
    assert_eq!(
        mapped.message(),
        "Audit logging failed: Failed to write audit log: disk full",
        "audit message must stay byte-identical to the Display output"
    );
    assert_eq!(mapped.message(), expected_message);
    Ok(())
}
