use serde_json::Value as JsonValue;
use std::path::Path;

use super::{ToolErrorKind, ToolHandlerError, builtin_tool::BuiltinTool};
use crate::bus::Bus;

#[derive(Debug, serde::Deserialize)]
struct GrepArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    glob: Option<String>,
    #[serde(default)]
    case_insensitive: bool,
    #[serde(default)]
    max_results: Option<usize>,
}

pub struct GrepTool;

impl BuiltinTool for GrepTool {
    const NAME: &'static str = "grep";

    async fn execute(
        args: &JsonValue,
        cwd: &Path,
        _bus: &Bus,
    ) -> Result<JsonValue, ToolHandlerError> {
        let args: GrepArgs =
            serde_json::from_value(args.clone()).map_err(|e| ToolHandlerError {
                kind: ToolErrorKind::Validation,
                message: format!("Invalid grep arguments: {e}"),
                details: None,
            })?;
        let search_path = if let Some(p) = args.path {
            super::resolve_fs_path_for_cwd(&p, cwd)
        } else {
            cwd.to_path_buf()
        };
        dispatch_grep(
            &args.pattern,
            &search_path,
            args.glob.as_deref(),
            args.case_insensitive,
            args.max_results.unwrap_or(200),
        )
    }
}

fn dispatch_grep(
    pattern: &str,
    search_path: &std::path::Path,
    glob_filter: Option<&str>,
    case_insensitive: bool,
    max_results: usize,
) -> Result<serde_json::Value, ToolHandlerError> {
    use ignore::WalkBuilder;
    use regex::RegexBuilder;

    let regex = RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|e| ToolHandlerError {
            kind: ToolErrorKind::Validation,
            message: format!("Invalid regex pattern: {e}"),
            details: None,
        })?;

    let mut walk = WalkBuilder::new(search_path);
    walk.standard_filters(true);

    if let Some(glob) = glob_filter {
        let mut ob = ignore::overrides::OverrideBuilder::new(search_path);
        ob.add(glob).map_err(|e| ToolHandlerError {
            kind: ToolErrorKind::Validation,
            message: format!("Invalid glob filter: {e}"),
            details: None,
        })?;
        let overrides = ob.build().map_err(|e| ToolHandlerError {
            kind: ToolErrorKind::Runtime,
            message: format!("Failed to build glob filter: {e}"),
            details: None,
        })?;
        walk.overrides(overrides);
    }

    let mut matches: Vec<serde_json::Value> = Vec::new();
    let mut truncated = false;

    'outer: for result in walk.build() {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            continue;
        }
        let path = entry.path();
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for (idx, line) in content.lines().enumerate() {
            if regex.is_match(line) {
                if matches.len() >= max_results {
                    truncated = true;
                    break 'outer;
                }
                let rel = path.strip_prefix(search_path).unwrap_or(path);
                matches.push(serde_json::json!({
                    "file": rel.to_string_lossy(),
                    "line": idx + 1,
                    "content": line,
                }));
            }
        }
    }

    let total = matches.len();
    Ok(serde_json::json!({
        "matches": matches,
        "total": total,
        "truncated": truncated,
    }))
}

#[cfg(test)]
#[path = "grep_test.rs"]
mod tests;
