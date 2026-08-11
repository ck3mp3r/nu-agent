use serde_json::Value as JsonValue;
use std::path::Path;

use super::ToolHandlerError;

#[derive(Debug, serde::Deserialize)]
struct SessionArgs {
    action: String,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    directory: Option<String>,
    #[serde(default)]
    force: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
struct WindowArgs {
    action: String,
    session: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    directory: Option<String>,
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    window: Option<String>,
    #[serde(default)]
    force: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
struct PaneArgs {
    action: String,
    session: String,
    #[serde(default)]
    pane: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    direction: Option<String>,
    #[serde(default)]
    size: Option<usize>,
    #[serde(default)]
    directory: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    lines: Option<usize>,
    #[serde(default)]
    force: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
struct LayoutArgs {
    action: String,
    session: String,
    window: String,
    layout: String,
}

/// Parse tool arguments into a typed struct, mapping serde errors to validation errors.
fn parse_args<T: serde::de::DeserializeOwned>(
    arguments: &JsonValue,
) -> Result<T, ToolHandlerError> {
    serde_json::from_value(arguments.clone())
        .map_err(|e| ToolHandlerError::validation(format!("Invalid tmux arguments: {e}")))
}

/// Run a `tmux` command and return its trimmed stdout.
fn run_tmux(args: &[&str]) -> Result<String, ToolHandlerError> {
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
fn resolve_dir(directory: Option<&str>, cwd: &Path) -> Option<String> {
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
fn require_force(force: Option<bool>) -> Result<(), ToolHandlerError> {
    if force == Some(true) {
        Ok(())
    } else {
        Err(ToolHandlerError::validation("kill requires force: true"))
    }
}

/// Build a pane target string, defaulting to the session's active pane.
fn pane_target(session: &str, pane: Option<&str>) -> String {
    match pane {
        Some(p) => format!("{session}:{p}"),
        None => session.to_string(),
    }
}

/// Parse a `WxH` size string into `(width, height)`.
fn parse_size(size: &str) -> (u64, u64) {
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
fn parse_sessions(output: &str) -> Vec<JsonValue> {
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
fn parse_windows(output: &str) -> Vec<JsonValue> {
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
fn parse_panes(output: &str) -> Vec<JsonValue> {
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
fn parse_panes_find(output: &str, name: Option<&str>, context: Option<&str>) -> Vec<JsonValue> {
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

fn dispatch_tmux_session(
    action: &str,
    args: &SessionArgs,
    cwd: &Path,
) -> Result<Option<JsonValue>, ToolHandlerError> {
    match action {
        "list" => {
            let output = run_tmux(&[
                "list-sessions",
                "-F",
                "#{session_name}|#{session_windows}|#{session_created}|#{session_attached}",
            ])?;
            Ok(Some(
                serde_json::json!({ "sessions": parse_sessions(&output) }),
            ))
        }
        "info" => {
            let session = args.session.as_deref().ok_or_else(|| {
                ToolHandlerError::validation("tmux_session info requires 'session'")
            })?;
            let output = run_tmux(&[
                "list-windows",
                "-t",
                session,
                "-F",
                "#{window_index}|#{window_name}|#{window_panes}|#{window_active}",
            ])?;
            Ok(Some(serde_json::json!({
                "session": session,
                "windows": parse_windows(&output),
            })))
        }
        "create" => {
            let name = args.name.as_deref().ok_or_else(|| {
                ToolHandlerError::validation("tmux_session create requires 'name'")
            })?;
            let mut cmd: Vec<String> = vec![
                "new-session".to_string(),
                "-d".to_string(),
                "-s".to_string(),
                name.to_string(),
            ];
            if let Some(dir) = resolve_dir(args.directory.as_deref(), cwd) {
                cmd.push("-c".to_string());
                cmd.push(dir);
            }
            let refs: Vec<&str> = cmd.iter().map(String::as_str).collect();
            run_tmux(&refs)?;
            Ok(Some(serde_json::json!({ "session": name })))
        }
        "kill" => {
            require_force(args.force)?;
            let session = args.session.as_deref().ok_or_else(|| {
                ToolHandlerError::validation("tmux_session kill requires 'session'")
            })?;
            run_tmux(&["kill-session", "-t", session])?;
            Ok(Some(serde_json::json!({ "killed": session })))
        }
        other => Err(ToolHandlerError::validation(format!(
            "Unknown tmux_session action '{other}'"
        ))),
    }
}

fn dispatch_tmux_window(
    action: &str,
    args: &WindowArgs,
    cwd: &Path,
) -> Result<Option<JsonValue>, ToolHandlerError> {
    match action {
        "create" => {
            let target = match args.index {
                Some(i) => format!("{}:{i}", args.session),
                None => args.session.clone(),
            };
            let mut cmd: Vec<String> = vec![
                "new-window".to_string(),
                "-t".to_string(),
                target,
                "-P".to_string(),
                "-F".to_string(),
                "#{window_name}".to_string(),
            ];
            if let Some(name) = &args.name {
                cmd.push("-n".to_string());
                cmd.push(name.clone());
            }
            if let Some(dir) = resolve_dir(args.directory.as_deref(), cwd) {
                cmd.push("-c".to_string());
                cmd.push(dir);
            }
            let refs: Vec<&str> = cmd.iter().map(String::as_str).collect();
            let output = run_tmux(&refs)?;
            let window_name = output.trim().to_string();
            Ok(Some(serde_json::json!({
                "session": args.session,
                "window": window_name,
            })))
        }
        "kill" => {
            require_force(args.force)?;
            let window = args.window.as_deref().ok_or_else(|| {
                ToolHandlerError::validation("tmux_window kill requires 'window'")
            })?;
            let target = format!("{}:{window}", args.session);
            run_tmux(&["kill-window", "-t", &target])?;
            Ok(Some(serde_json::json!({ "killed": target })))
        }
        other => Err(ToolHandlerError::validation(format!(
            "Unknown tmux_window action '{other}'"
        ))),
    }
}

fn dispatch_tmux_pane(
    action: &str,
    args: &PaneArgs,
    cwd: &Path,
) -> Result<Option<JsonValue>, ToolHandlerError> {
    match action {
        "list" => {
            let output = run_tmux(&[
                "list-panes",
                "-t",
                args.session.as_str(),
                "-F",
                "#{pane_id}|#{pane_index}|#{pane_active}|#{pane_current_command}|#{pane_title}|#{pane_width}x#{pane_height}",
            ])?;
            Ok(Some(serde_json::json!({ "panes": parse_panes(&output) })))
        }
        "find" => {
            let name = args.name.as_deref();
            let context = args.context.as_deref();
            if name.is_none() && context.is_none() {
                return Err(ToolHandlerError::validation(
                    "tmux_pane find requires 'name' or 'context'",
                ));
            }
            let output = run_tmux(&[
                "list-panes",
                "-t",
                args.session.as_str(),
                "-F",
                "#{pane_id}|#{pane_index}|#{pane_active}|#{pane_current_command}|#{pane_title}|#{pane_width}x#{pane_height}|#{pane_current_path}",
            ])?;
            Ok(Some(serde_json::json!({
                "panes": parse_panes_find(&output, name, context),
            })))
        }
        "process" => {
            let target = pane_target(&args.session, args.pane.as_deref());
            let output = run_tmux(&["display-message", "-t", &target, "-p", "#{pane_pid}"])?;
            let pid = output.trim().parse::<u64>().map_err(|_| {
                ToolHandlerError::runtime("failed to parse pane PID from tmux output")
            })?;
            Ok(Some(serde_json::json!({ "pid": pid })))
        }
        "capture" => {
            let target = pane_target(&args.session, args.pane.as_deref());
            let mut cmd: Vec<String> = vec![
                "capture-pane".to_string(),
                "-t".to_string(),
                target.clone(),
                "-p".to_string(),
            ];
            if let Some(lines) = args.lines {
                cmd.push("-S".to_string());
                cmd.push(format!("-{lines}"));
            }
            let refs: Vec<&str> = cmd.iter().map(String::as_str).collect();
            let output = run_tmux(&refs)?;
            Ok(Some(serde_json::json!({ "content": output })))
        }
        "send" => {
            let command = args
                .command
                .as_deref()
                .ok_or_else(|| ToolHandlerError::validation("tmux_pane send requires 'command'"))?;
            let target = pane_target(&args.session, args.pane.as_deref());
            run_tmux(&["send-keys", "-t", &target, command, "Enter"])?;
            let output = run_tmux(&["capture-pane", "-t", &target, "-p"])?;
            Ok(Some(serde_json::json!({ "content": output })))
        }
        "split" => {
            let target = pane_target(&args.session, args.pane.as_deref());
            let mut cmd: Vec<String> = vec![
                "split-window".to_string(),
                "-t".to_string(),
                target.clone(),
                "-P".to_string(),
                "-F".to_string(),
                "#{pane_id}".to_string(),
            ];
            match args.direction.as_deref() {
                Some("horizontal") => cmd.push("-h".to_string()),
                Some("vertical") => cmd.push("-v".to_string()),
                Some(other) => {
                    return Err(ToolHandlerError::validation(format!(
                        "Invalid tmux_pane split direction '{other}': expected 'horizontal' or 'vertical'"
                    )));
                }
                None => {}
            }
            if let Some(size) = args.size {
                cmd.push("-p".to_string());
                cmd.push(size.to_string());
            }
            if let Some(dir) = resolve_dir(args.directory.as_deref(), cwd) {
                cmd.push("-c".to_string());
                cmd.push(dir);
            }
            let refs: Vec<&str> = cmd.iter().map(String::as_str).collect();
            let output = run_tmux(&refs)?;
            let new_pane = output.trim().to_string();
            Ok(Some(serde_json::json!({
                "session": args.session,
                "pane": new_pane,
            })))
        }
        "kill" => {
            require_force(args.force)?;
            let target = pane_target(&args.session, args.pane.as_deref());
            run_tmux(&["kill-pane", "-t", &target])?;
            Ok(Some(serde_json::json!({ "killed": target })))
        }
        other => Err(ToolHandlerError::validation(format!(
            "Unknown tmux_pane action '{other}'"
        ))),
    }
}

fn dispatch_tmux_layout(
    action: &str,
    args: &LayoutArgs,
) -> Result<Option<JsonValue>, ToolHandlerError> {
    match action {
        "select" => {
            let target = format!("{}:{}", args.session, args.window);
            run_tmux(&["select-layout", "-t", &target, &args.layout])?;
            Ok(Some(serde_json::json!({
                "session": args.session,
                "window": args.window,
                "layout": args.layout,
            })))
        }
        other => Err(ToolHandlerError::validation(format!(
            "Unknown tmux_layout action '{other}'"
        ))),
    }
}

/// Dispatch a tmux tool call to the appropriate sub-handler.
pub fn dispatch_tmux_tool(
    tool_name: &str,
    arguments: &JsonValue,
    cwd: &Path,
) -> Result<Option<JsonValue>, ToolHandlerError> {
    match tool_name {
        "tmux_session" => {
            let args: SessionArgs = parse_args(arguments)?;
            dispatch_tmux_session(&args.action, &args, cwd)
        }
        "tmux_window" => {
            let args: WindowArgs = parse_args(arguments)?;
            dispatch_tmux_window(&args.action, &args, cwd)
        }
        "tmux_pane" => {
            let args: PaneArgs = parse_args(arguments)?;
            dispatch_tmux_pane(&args.action, &args, cwd)
        }
        "tmux_layout" => {
            let args: LayoutArgs = parse_args(arguments)?;
            dispatch_tmux_layout(&args.action, &args)
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
#[path = "tmux_test.rs"]
mod tests;
