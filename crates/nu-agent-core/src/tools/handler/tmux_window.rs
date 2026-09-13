use serde_json::Value as JsonValue;
use std::path::Path;

use super::tmux_common::{parse_args, require_force, resolve_dir, run_tmux};
use super::{ToolHandlerError, builtin_tool::BuiltinTool};
use crate::bus::Bus;

#[derive(Debug, serde::Deserialize)]
pub struct WindowArgs {
    pub action: String,
    pub session: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(default)]
    pub index: Option<usize>,
    #[serde(default)]
    pub window: Option<String>,
    #[serde(default)]
    pub force: Option<bool>,
}

pub struct TmuxWindowTool;

/// Parse `tmux new-window` `#{window_id}:#{window_index}` output into `(id, index)`.
pub(crate) fn parse_window_create_output(output: &str) -> Option<(String, u64)> {
    let trimmed = output.trim();
    let (id, index) = trimmed.split_once(':')?;
    let index = index.parse::<u64>().ok()?;
    Some((id.to_string(), index))
}

impl BuiltinTool for TmuxWindowTool {
    const NAME: &'static str = "tmux_window";

    async fn execute(
        arguments: &JsonValue,
        cwd: &Path,
        _bus: &Bus,
    ) -> Result<JsonValue, ToolHandlerError> {
        let args: WindowArgs = parse_args(arguments)?;

        match args.action.as_str() {
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
                    "#{window_id}:#{window_index}".to_string(),
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
                let (window_id, window_index) =
                    parse_window_create_output(&output).ok_or_else(|| {
                        ToolHandlerError::runtime(format!(
                            "Unexpected tmux new-window output: {}",
                            output.trim()
                        ))
                    })?;
                let mut response = serde_json::json!({
                    "session": args.session,
                    "window_id": window_id,
                    "window_index": window_index,
                });
                if let Some(name) = &args.name {
                    response["window_name"] = serde_json::json!(name);
                }
                Ok(response)
            }
            "kill" => {
                require_force(args.force)?;
                let window = args.window.as_deref().ok_or_else(|| {
                    ToolHandlerError::validation("tmux_window kill requires 'window'")
                })?;
                let target = format!("{}:{window}", args.session);
                run_tmux(&["kill-window", "-t", &target])?;
                Ok(serde_json::json!({ "killed": target }))
            }
            other => Err(ToolHandlerError::validation(format!(
                "Unknown tmux_window action '{other}'"
            ))),
        }
    }
}

#[cfg(test)]
#[path = "tmux_window_test.rs"]
mod tests;
