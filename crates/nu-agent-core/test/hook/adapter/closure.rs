use super::*;

use crate::tools::audit::AuditError;
use crate::tools::error::ToolError;
use nu_protocol::shell_error::generic::GenericError;
use nu_protocol::{ShellError, Span};
use std::time::Duration;

/// Compile-time check that ClosureToolAdapter implements Send + Sync.
///
/// This test ensures that our adapter can be safely shared across threads.
#[test]
fn closure_tool_adapter_is_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    assert_send::<ClosureToolAdapter>();
    assert_sync::<ClosureToolAdapter>();
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

// ================================================================
// ToolError → ToolExecutionError mapping
// ================================================================

type TestResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;

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

// Note: More comprehensive tests for `name()`, `definition()`, and `call()`
// would require setting up a full Nushell engine environment with:
// - A running nu_plugin EngineInterface
// - A ToolExecutor with audit logger
// - Valid Spanned<Closure> instances
//
// These tests would be complex integration tests. For now, we verify the critical
// trait bounds (Send + Sync) and rely on manual testing with the full engine.
//
// TODO: Add integration tests that:
// 1. Create a mock EngineInterface
// 2. Build a simple closure (e.g., {|x| $x + 1})
// 3. Verify the DynamicTool has the correct name
// 4. Verify the DynamicTool has a valid definition
// 5. Verify execution via ToolSet executes the closure and returns correct JSON
