use super::audit::AuditError;
use nu_protocol::ShellError;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Tool '{tool_name}' timed out after {timeout:?}")]
    Timeout {
        tool_name: String,
        timeout: Duration,
    },

    #[error("Tool execution failed: {0}")]
    Execution(#[from] ShellError),

    #[error("Audit logging failed: {0}")]
    Audit(#[from] AuditError),
}
