use serde_json::Value as JsonValue;
use std::path::Path;

use super::tmux_common::{parse_args, run_tmux};
use super::{ToolHandlerError, builtin_tool::BuiltinTool};
use crate::bus::Bus;

#[derive(Debug, serde::Deserialize)]
pub struct LayoutArgs {
    pub action: String,
    pub session: String,
    pub window: String,
    pub layout: String,
}

pub struct TmuxLayoutTool;

impl BuiltinTool for TmuxLayoutTool {
    const NAME: &'static str = "tmux_layout";

    async fn execute(
        arguments: &JsonValue,
        _cwd: &Path,
        _bus: &Bus,
    ) -> Result<JsonValue, ToolHandlerError> {
        let args: LayoutArgs = parse_args(arguments)?;

        match args.action.as_str() {
            "select" => {
                let target = format!("{}:{}", args.session, args.window);
                run_tmux(&["select-layout", "-t", &target, &args.layout])?;
                Ok(serde_json::json!({
                    "session": args.session,
                    "window": args.window,
                    "layout": args.layout,
                }))
            }
            other => Err(ToolHandlerError::validation(format!(
                "Unknown tmux_layout action '{other}'"
            ))),
        }
    }
}

#[cfg(test)]
#[path = "tmux_layout_test.rs"]
mod tests;
