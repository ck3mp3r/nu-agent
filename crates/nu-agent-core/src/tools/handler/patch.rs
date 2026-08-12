use serde_json::Value as JsonValue;
use std::path::Path;

use super::edit::map_mutate_error;
use super::{ToolErrorKind, ToolHandlerError, builtin_tool::BuiltinTool};
use crate::bus::Bus;
use crate::tools::fs::core::{PatchOp, PatchRange, apply_line_range_patch_batch};

#[derive(Debug, serde::Deserialize)]
struct PatchRangeArgs {
    start: usize,
    end: usize,
}

#[derive(Debug, serde::Deserialize)]
struct PatchOpArgs {
    range: PatchRangeArgs,
    replacement: String,
}

#[derive(Debug, serde::Deserialize)]
struct PatchArgs {
    path: String,
    #[serde(default)]
    expected_version: Option<String>,
    operations: Vec<PatchOpArgs>,
}

pub struct PatchTool;

impl BuiltinTool for PatchTool {
    const NAME: &'static str = "patch";

    async fn execute(
        args: &JsonValue,
        cwd: &Path,
        _bus: &Bus,
    ) -> Result<JsonValue, ToolHandlerError> {
        let args: PatchArgs =
            serde_json::from_value(args.clone()).map_err(|e| ToolHandlerError {
                kind: ToolErrorKind::Validation,
                message: format!("Invalid patch arguments: {e}"),
                details: None,
            })?;

        let resolved_path = super::resolve_fs_path_for_cwd(&args.path, cwd);
        let operations = args
            .operations
            .into_iter()
            .map(|op| PatchOp {
                range: PatchRange::new(op.range.start, op.range.end),
                replacement: op.replacement,
            })
            .collect::<Vec<_>>();

        let summary = apply_line_range_patch_batch(
            &resolved_path,
            args.expected_version.as_deref(),
            operations,
        )
        .map_err(map_mutate_error)?;

        Ok(serde_json::json!({
            "path": args.path,
            "operation_count": summary.operation_count,
            "applied_ranges": summary
                .applied_ranges
                .iter()
                .map(|range| serde_json::json!({"start": range.start, "end": range.end}))
                .collect::<Vec<_>>(),
            "wrote": summary.wrote,
            "changed": summary.changed,
            "noop": summary.noop,
            "conflict": summary.conflict,
            "expected_version": summary.expected_version,
            "previous_version": summary.previous_version,
            "new_version": summary.new_version,
            "previous_lines": summary.previous_lines,
            "new_lines": summary.new_lines,
        }))
    }
}

#[cfg(test)]
#[path = "patch_test.rs"]
mod tests;
