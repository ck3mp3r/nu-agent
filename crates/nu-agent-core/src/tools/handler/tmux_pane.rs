use serde_json::Value as JsonValue;
use std::path::Path;

use super::tmux_common::{
    pane_target, parse_args, parse_panes, parse_panes_find, require_force, resolve_dir, run_tmux,
};
use super::{ToolHandlerError, builtin_tool::BuiltinTool};
use crate::bus::Bus;

#[derive(Debug, serde::Deserialize)]
pub struct PaneArgs {
    pub action: String,
    pub session: String,
    #[serde(default)]
    pub pane: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub size: Option<usize>,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub lines: Option<usize>,
    #[serde(default)]
    pub force: Option<bool>,
}

pub struct TmuxPaneTool;

impl BuiltinTool for TmuxPaneTool {
    const NAME: &'static str = "tmux_pane";

    async fn execute(
        arguments: &JsonValue,
        cwd: &Path,
        _bus: &Bus,
    ) -> Result<JsonValue, ToolHandlerError> {
        let args: PaneArgs = parse_args(arguments)?;

        match args.action.as_str() {
            "list" => {
                let output = run_tmux(&[
                    "list-panes",
                    "-t",
                    args.session.as_str(),
                    "-F",
                    "#{pane_id}|#{pane_index}|#{pane_active}|#{pane_current_command}|#{pane_title}|#{pane_width}x#{pane_height}",
                ])?;
                Ok(serde_json::json!({ "panes": parse_panes(&output) }))
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
                Ok(serde_json::json!({
                    "panes": parse_panes_find(&output, name, context),
                }))
            }
            "process" => {
                let target = pane_target(&args.session, args.pane.as_deref());
                let output = run_tmux(&["display-message", "-t", &target, "-p", "#{pane_pid}"])?;
                let pid = output.trim().parse::<u64>().map_err(|_| {
                    ToolHandlerError::runtime("failed to parse pane PID from tmux output")
                })?;
                Ok(serde_json::json!({ "pid": pid }))
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
                Ok(serde_json::json!({ "content": output }))
            }
            "send" => {
                let command = args.command.as_deref().ok_or_else(|| {
                    ToolHandlerError::validation("tmux_pane send requires 'command'")
                })?;
                let target = pane_target(&args.session, args.pane.as_deref());
                run_tmux(&["send-keys", "-t", &target, command, "Enter"])?;
                let output = run_tmux(&["capture-pane", "-t", &target, "-p"])?;
                Ok(serde_json::json!({ "content": output }))
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
                Ok(serde_json::json!({
                    "session": args.session,
                    "pane": new_pane,
                }))
            }
            "kill" => {
                require_force(args.force)?;
                let target = pane_target(&args.session, args.pane.as_deref());
                run_tmux(&["kill-pane", "-t", &target])?;
                Ok(serde_json::json!({ "killed": target }))
            }
            other => Err(ToolHandlerError::validation(format!(
                "Unknown tmux_pane action '{other}'"
            ))),
        }
    }
}

#[cfg(test)]
#[path = "tmux_pane_test.rs"]
mod tests;
