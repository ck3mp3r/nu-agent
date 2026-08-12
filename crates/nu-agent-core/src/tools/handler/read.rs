use serde_json::Value as JsonValue;
use std::path::Path;

use super::{ToolHandlerError, builtin_tool::BuiltinTool};
use crate::bus::Bus;

#[derive(Debug, serde::Deserialize)]
struct ReadArgs {
    path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

pub struct ReadTool;

impl BuiltinTool for ReadTool {
    const NAME: &'static str = "read";

    async fn execute(
        args: &JsonValue,
        cwd: &Path,
        _bus: &Bus,
    ) -> Result<JsonValue, ToolHandlerError> {
        let args: ReadArgs = serde_json::from_value(args.clone())
            .map_err(|e| ToolHandlerError::validation(format!("Invalid read arguments: {e}")))?;

        let resolved_path = super::resolve_fs_path_for_cwd(&args.path, cwd);

        use crate::tools::fs::core::{ReadRequest, read_file};
        let response = read_file(
            &resolved_path,
            ReadRequest {
                offset: args.offset,
                limit: args.limit,
            },
        )
        .map_err(|e| ToolHandlerError::runtime(format!("read failed: {e}")))?;

        Ok(serde_json::json!({
            "path": args.path,
            "content": response.content,
            "total_lines": response.total_lines,
            "offset": response.offset,
            "limit": response.limit,
            "version": response.version,
        }))
    }
}

#[cfg(test)]
#[path = "read_test.rs"]
mod tests;
