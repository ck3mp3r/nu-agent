//! The structural tool status: the disposition map.
//!
//! No test here reads a result's text: the classification input is the
//! structured disposition. The status the turn settles is recorded in the
//! settle transaction, whose round trip the storage suite covers.

use crate::session::tool_status::{ToolStatus, status_of_result};
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
