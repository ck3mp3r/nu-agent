//! The structural tool status: the disposition map.
//!
//! No test here reads a result's text: the classification input is the
//! structured disposition. The status the turn settles is recorded in the
//! settle transaction, whose round trip the storage suite covers.

use crate::session::tool_status::{ToolStatus, error_kind_of, status_of_result};
use crate::types::{ToolExecutionError, ToolExecutionResult, ToolOutput};

pub(crate) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// WHEN a call settles, THE status SHALL come from the structured disposition:
/// a success, a policy skip (the permission denial), a cancelled error kind,
/// and everything else a failure.
#[test]
fn test_tool_status_status_of_result_maps_every_disposition() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, ToolExecutionResult, ToolStatus)> = vec![
        (
            "success",
            ToolExecutionResult::success(ToolOutput::text("ok")),
            ToolStatus::Success,
        ),
        (
            "cancelled kind",
            ToolExecutionResult::failed(ToolExecutionError::cancelled("stopped")),
            ToolStatus::Cancelled,
        ),
        (
            "runtime failure",
            ToolExecutionResult::failed(ToolExecutionError::other("broke")),
            ToolStatus::Failed,
        ),
        (
            "intentional refusal",
            ToolExecutionResult::failed(ToolExecutionError::refused("no")),
            ToolStatus::Failed,
        ),
        (
            "policy skip (the permission denial)",
            ToolExecutionResult::skipped("denied by policy"),
            ToolStatus::Cancelled,
        ),
    ];

    // -- Exec & Check
    for (label, result, expected) in cases {
        assert_eq!(status_of_result(&result), expected, "{label}");
    }
    Ok(())
}

/// WHEN a call fails with a rig error, THE record SHALL carry the matching
/// enumerated kind — the classification travels as data, so a surface prints
/// what the record says instead of parsing the message.
#[test]
fn test_tool_status_error_kind_of_maps_every_constructible_kind() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        (
            ToolExecutionError::invalid_args("bad args"),
            agnt_kernel::ToolErrorKind::InvalidArgs,
        ),
        (
            ToolExecutionError::timeout("slow"),
            agnt_kernel::ToolErrorKind::Timeout,
        ),
        (
            ToolExecutionError::cancelled("stopped"),
            agnt_kernel::ToolErrorKind::Cancelled,
        ),
        (
            ToolExecutionError::permission_denied("refused"),
            agnt_kernel::ToolErrorKind::PermissionDenied,
        ),
        (
            ToolExecutionError::network("unreachable"),
            agnt_kernel::ToolErrorKind::Network,
        ),
        (
            ToolExecutionError::other("broke"),
            agnt_kernel::ToolErrorKind::Other,
        ),
    ];

    // -- Exec & Check
    for (error, expected) in cases {
        let result = ToolExecutionResult::failed(error);
        let kind = result.error().map(|e| error_kind_of(e.kind()));
        assert_eq!(kind, Some(expected), "a failed call must classify itself");
    }
    Ok(())
}
