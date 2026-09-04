//! Tests for `FilteredToolProxy`'s tool-result mapping (`map_tool_result`).

use rig::tool::{ToolExecutionError, ToolOutput, ToolResult};

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A successful tool result maps to its output unchanged.
#[test]
fn map_tool_result_success_returns_output_unchanged() -> Result<()> {
    // -- Setup & Fixtures
    let result = ToolResult::success(ToolOutput::text("file contents"));

    // -- Exec
    let output = map_tool_result(&result)?;

    // -- Check
    assert_eq!(output.as_text(), Some("file contents"));
    Ok(())
}

/// A refused tool result maps to the exact `[refused]` marker text.
#[test]
fn map_tool_result_refusal_returns_refused_marker() -> Result<()> {
    // -- Setup & Fixtures
    let result = ToolResult::failed(ToolExecutionError::refused("user declined the call"));

    // -- Exec
    let output = map_tool_result(&result).map_err(|_| "refusal must not map to Err")?;

    // -- Check
    assert_eq!(output.as_text(), Some("[refused]"));
    Ok(())
}

/// A skipped tool result also maps to the exact `[refused]` marker text.
#[test]
fn map_tool_result_skipped_returns_refused_marker() -> Result<()> {
    // -- Setup & Fixtures
    let result = ToolResult::skipped("skipped by runtime policy");

    // -- Exec
    let output = map_tool_result(&result).map_err(|_| "skip must not map to Err")?;

    // -- Check
    assert_eq!(output.as_text(), Some("[refused]"));
    Ok(())
}

/// An errored tool result maps to `Err` with the structured error preserved.
#[test]
fn map_tool_result_error_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let result = ToolResult::failed(ToolExecutionError::other("boom"));

    // -- Exec
    let error = map_tool_result(&result)
        .err()
        .ok_or("error result must map to Err")?;

    // -- Check
    assert!(
        error.to_string().contains("boom"),
        "error must preserve the execution error message, got: {error}"
    );
    Ok(())
}
