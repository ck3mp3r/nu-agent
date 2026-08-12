use serde_json::Value as JsonValue;
use std::path::Path;

use super::{ToolErrorKind, ToolHandlerError, builtin_tool::BuiltinTool};
use crate::bus::Bus;

#[derive(Debug, serde::Deserialize)]
struct GlobArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

pub struct GlobTool;

impl BuiltinTool for GlobTool {
    const NAME: &'static str = "glob";

    async fn execute(
        args: &JsonValue,
        cwd: &Path,
        _bus: &Bus,
    ) -> Result<JsonValue, ToolHandlerError> {
        let args: GlobArgs =
            serde_json::from_value(args.clone()).map_err(|e| ToolHandlerError {
                kind: ToolErrorKind::Validation,
                message: format!("Invalid glob arguments: {e}"),
                details: None,
            })?;
        let search_path = if let Some(ref p) = args.path {
            super::resolve_fs_path_for_cwd(p, cwd)
        } else {
            cwd.to_path_buf()
        };
        dispatch_glob(&args.pattern, &search_path)
    }
}

fn dispatch_glob(
    pattern: &str,
    search_path: &std::path::Path,
) -> Result<serde_json::Value, ToolHandlerError> {
    use ignore::WalkBuilder;
    use ignore::overrides::OverrideBuilder;

    let mut ob = OverrideBuilder::new(search_path);
    ob.add(pattern).map_err(|e| ToolHandlerError {
        kind: ToolErrorKind::Validation,
        message: format!("Invalid glob pattern: {e}"),
        details: None,
    })?;
    let overrides = ob.build().map_err(|e| ToolHandlerError {
        kind: ToolErrorKind::Runtime,
        message: format!("Failed to build glob: {e}"),
        details: None,
    })?;

    let mut matches: Vec<String> = Vec::new();

    for result in WalkBuilder::new(search_path)
        .standard_filters(true)
        .overrides(overrides)
        .build()
    {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(search_path)
            .unwrap_or(entry.path());
        matches.push(rel.to_string_lossy().into_owned());
    }

    matches.sort();
    let total = matches.len();
    Ok(serde_json::json!({
        "matches": matches,
        "total": total,
    }))
}

#[cfg(test)]
#[path = "glob_test.rs"]
mod tests;
