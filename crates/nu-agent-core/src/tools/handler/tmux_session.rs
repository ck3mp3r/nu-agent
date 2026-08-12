use serde_json::Value as JsonValue;
use std::path::Path;

use super::tmux_common::{
    parse_args, parse_sessions, parse_windows, require_force, resolve_dir, run_tmux,
};
use super::{ToolHandlerError, builtin_tool::BuiltinTool};
use crate::bus::Bus;

#[derive(Debug, serde::Deserialize)]
pub struct SessionArgs {
    pub action: String,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(default)]
    pub force: Option<bool>,
}

pub struct TmuxSessionTool;

impl BuiltinTool for TmuxSessionTool {
    const NAME: &'static str = "tmux_session";

    async fn execute(
        arguments: &JsonValue,
        cwd: &Path,
        _bus: &Bus,
    ) -> Result<JsonValue, ToolHandlerError> {
        let args: SessionArgs = parse_args(arguments)?;

        match args.action.as_str() {
            "list" => {
                let output = run_tmux(&[
                    "list-sessions",
                    "-F",
                    "#{session_name}|#{session_windows}|#{session_created}|#{session_attached}",
                ])?;
                Ok(serde_json::json!({ "sessions": parse_sessions(&output) }))
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
                Ok(serde_json::json!({
                    "session": session,
                    "windows": parse_windows(&output),
                }))
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
                Ok(serde_json::json!({ "session": name }))
            }
            "kill" => {
                require_force(args.force)?;
                let session = args.session.as_deref().ok_or_else(|| {
                    ToolHandlerError::validation("tmux_session kill requires 'session'")
                })?;
                run_tmux(&["kill-session", "-t", session])?;
                Ok(serde_json::json!({ "killed": session }))
            }
            other => Err(ToolHandlerError::validation(format!(
                "Unknown tmux_session action '{other}'"
            ))),
        }
    }
}

#[cfg(test)]
#[path = "tmux_session_test.rs"]
mod tests;
