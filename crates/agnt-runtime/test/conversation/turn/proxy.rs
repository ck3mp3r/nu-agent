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

/// A refused tool result maps to `Err` carrying the original refusal error —
/// refusals stay failure-shaped instead of re-entering the pipeline
/// success-shaped behind the `[refused]` marker.
#[test]
fn map_tool_result_refusal_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let result = ToolResult::failed(ToolExecutionError::refused("user declined the call"));

    // -- Exec
    let error = map_tool_result(&result)
        .err()
        .ok_or("refusal must map to Err")?;

    // -- Check
    assert!(
        error.is_refusal(),
        "refusal disposition must survive the mapping"
    );
    assert!(
        error.message().contains("user declined the call"),
        "refusal must preserve the original refusal message, got: {}",
        error.message()
    );
    Ok(())
}

/// A skipped tool result maps to `Err` with a refusal-classified error whose
/// model feedback is the exact `[refused]` marker — the state is structural,
/// the marker survives as model feedback only.
#[test]
fn map_tool_result_skipped_returns_refusal_classified_error() -> Result<()> {
    // -- Setup & Fixtures
    let result = ToolResult::skipped("skipped by runtime policy");

    // -- Exec
    let error = map_tool_result(&result)
        .err()
        .ok_or("skip must map to Err")?;

    // -- Check
    assert_eq!(
        error.kind(),
        rig::tool::ToolErrorKind::PermissionDenied,
        "synthesized skip error must be refusal-classified"
    );
    assert!(
        error.is_refusal(),
        "synthesized skip error must carry the refusal disposition"
    );
    assert_eq!(
        error.model_feedback(),
        Some("[refused]"),
        "model feedback must remain the exact [refused] marker"
    );
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
