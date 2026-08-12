use serde_json::Value as JsonValue;
use std::path::Path;

use super::ToolHandlerError;

/// Parse tool arguments into a typed struct, mapping serde errors to validation errors.
pub(crate) fn parse_args<T: serde::de::DeserializeOwned>(
    arguments: &JsonValue,
) -> Result<T, ToolHandlerError> {
    serde_json::from_value(arguments.clone())
        .map_err(|e| ToolHandlerError::validation(format!("Invalid tmux arguments: {e}")))
}

/// Run a `tmux` command and return its trimmed stdout.
pub(crate) fn run_tmux(args: &[&str]) -> Result<String, ToolHandlerError> {
    let output = std::process::Command::new("tmux")
        .args(args)
        .output()
        .map_err(|e| ToolHandlerError::runtime(format!("Failed to run tmux: {e}")))?;

    if !output.status.success() {
        return Err(ToolHandlerError::runtime(format!(
            "tmux {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Resolve a possibly-relative directory against the tool's working directory.
pub(crate) fn resolve_dir(directory: Option<&str>, cwd: &Path) -> Option<String> {
    directory.map(|d| {
        let p = Path::new(d);
        if p.is_absolute() {
            d.to_string()
        } else {
            cwd.join(p).to_string_lossy().to_string()
        }
    })
}

/// Require `force: true` for destructive operations.
pub(crate) fn require_force(force: Option<bool>) -> Result<(), ToolHandlerError> {
    if force == Some(true) {
        Ok(())
    } else {
        Err(ToolHandlerError::validation("kill requires force: true"))
    }
}

/// Build a pane target string, defaulting to the session's active pane.
pub(crate) fn pane_target(session: &str, pane: Option<&str>) -> String {
    match pane {
        Some(p) if p.starts_with('%') => p.to_string(),
        Some(p) => format!("{session}:{p}"),
        None => session.to_string(),
    }
}

/// Parse a `WxH` size string into `(width, height)`.
pub(crate) fn parse_size(size: &str) -> (u64, u64) {
    let mut parts = size.split('x');
    let width = parts
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let height = parts
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    (width, height)
}

/// Parse `tmux list-sessions` pipe-delimited output into a JSON array.
pub(crate) fn parse_sessions(output: &str) -> Vec<JsonValue> {
    output
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let fields: Vec<&str> = line.split('|').collect();
            serde_json::json!({
                "name": fields.first().copied().unwrap_or(""),
                "windows": fields.get(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
                "created": fields.get(2).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0),
                "attached": fields.get(3).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
            })
        })
        .collect()
}

/// Parse `tmux list-windows` pipe-delimited output into a JSON array.
pub(crate) fn parse_windows(output: &str) -> Vec<JsonValue> {
    output
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let fields: Vec<&str> = line.split('|').collect();
            serde_json::json!({
                "index": fields.first().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
                "name": fields.get(1).copied().unwrap_or(""),
                "panes": fields.get(2).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
                "active": fields.get(3).map(|s| *s == "1").unwrap_or(false),
            })
        })
        .collect()
}

/// Parse `tmux list-panes` pipe-delimited output into a JSON array.
pub(crate) fn parse_panes(output: &str) -> Vec<JsonValue> {
    output
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let fields: Vec<&str> = line.split('|').collect();
            let (width, height) = parse_size(fields.get(5).copied().unwrap_or(""));
            serde_json::json!({
                "id": fields.first().copied().unwrap_or(""),
                "index": fields.get(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
                "active": fields.get(2).map(|s| *s == "1").unwrap_or(false),
                "command": fields.get(3).copied().unwrap_or(""),
                "title": fields.get(4).copied().unwrap_or(""),
                "width": width,
                "height": height,
            })
        })
        .collect()
}

/// Parse `tmux list-panes` output (with current path) and filter by name/context.
pub(crate) fn parse_panes_find(
    output: &str,
    name: Option<&str>,
    context: Option<&str>,
) -> Vec<JsonValue> {
    output
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('|').collect();
            let title = fields.get(4).copied().unwrap_or("");
            let command = fields.get(3).copied().unwrap_or("");
            let path = fields.get(6).copied().unwrap_or("");

            let name_match = name.is_none_or(|n| title.contains(n));
            let context_match = context.is_none_or(|c| path.contains(c) || command.contains(c));

            if !name_match || !context_match {
                return None;
            }

            let (width, height) = parse_size(fields.get(5).copied().unwrap_or(""));
            Some(serde_json::json!({
                "id": fields.first().copied().unwrap_or(""),
                "index": fields.get(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
                "active": fields.get(2).map(|s| *s == "1").unwrap_or(false),
                "command": command,
                "title": title,
                "path": path,
                "width": width,
                "height": height,
            }))
        })
        .collect()
}

#[cfg(test)]
#[path = "tmux_test.rs"]
mod tests;
