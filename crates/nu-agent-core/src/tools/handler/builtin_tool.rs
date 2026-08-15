use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use super::ToolHandlerError;
use super::builtin_kinds::BuiltinKind;
use super::edit::EditTool;
use super::glob::GlobTool;
use super::grep::GrepTool;
use super::http::HttpTool;
use super::nu::NuTool;
use super::patch::PatchTool;
use super::read::ReadTool;
use super::skill::SkillTool;
use super::tmux_layout::TmuxLayoutTool;
use super::tmux_pane::TmuxPaneTool;
use super::tmux_session::TmuxSessionTool;
use super::tmux_window::TmuxWindowTool;
use super::tree_sitter::{AstNodesTool, AstQueryTool, AstRefsTool, AstTreeTool};
use crate::bus::Bus;
use crate::tools::limits::truncate_tool_output;
use crate::types::ToolDefinition;
use rig::tool::server::ToolServerHandle;
use rig::tool::{DynamicTool, ToolExecutionError, ToolOutput};

pub trait BuiltinTool: Sized {
    const NAME: &'static str;
    fn execute(
        args: &JsonValue,
        cwd: &Path,
        bus: &Bus,
    ) -> impl std::future::Future<Output = Result<JsonValue, ToolHandlerError>> + Send;
}

pub fn make_dynamic_tool<T: BuiltinTool>(
    def: ToolDefinition,
    cwd: PathBuf,
    max_bytes: usize,
    bus: Bus,
) -> DynamicTool {
    let name = def.name.clone();
    let description = def.description.clone();
    let parameters = def.parameters.clone();
    DynamicTool::new(name, description, parameters, move |_ctx, args| {
        let cwd = cwd.clone();
        let bus = bus.clone();
        Box::pin(async move {
            let result = T::execute(&args, &cwd, &bus).await.map_err(|e| {
                ToolExecutionError::provider(format!(
                    "{}: {}",
                    e.message,
                    e.details
                        .map(|d| d.to_string())
                        .unwrap_or_else(|| "no details".to_string())
                ))
            })?;
            let json_str = serde_json::to_string(&result).map_err(|e| {
                ToolExecutionError::other(format!("JSON serialization failed: {e}"))
            })?;
            Ok(ToolOutput::text(truncate_tool_output(json_str, max_bytes)))
        })
    })
}

pub async fn register_builtin(
    def: ToolDefinition,
    cwd: PathBuf,
    max_bytes: usize,
    bus: Bus,
    tool_server: &ToolServerHandle,
) {
    let kind = match BuiltinKind::from_str(&def.name) {
        Ok(k) => k,
        Err(_) => return,
    };
    let tool = match kind {
        BuiltinKind::Read => make_dynamic_tool::<ReadTool>(def, cwd, max_bytes, bus),
        BuiltinKind::Edit => make_dynamic_tool::<EditTool>(def, cwd, max_bytes, bus),
        BuiltinKind::Patch => make_dynamic_tool::<PatchTool>(def, cwd, max_bytes, bus),
        BuiltinKind::Skill => make_dynamic_tool::<SkillTool>(def, cwd, max_bytes, bus),
        BuiltinKind::Grep => make_dynamic_tool::<GrepTool>(def, cwd, max_bytes, bus),
        BuiltinKind::Glob => make_dynamic_tool::<GlobTool>(def, cwd, max_bytes, bus),
        BuiltinKind::Http => make_dynamic_tool::<HttpTool>(def, cwd, max_bytes, bus),
        BuiltinKind::Nu => make_dynamic_tool::<NuTool>(def, cwd, max_bytes, bus),
        BuiltinKind::TmuxSession => make_dynamic_tool::<TmuxSessionTool>(def, cwd, max_bytes, bus),
        BuiltinKind::TmuxWindow => make_dynamic_tool::<TmuxWindowTool>(def, cwd, max_bytes, bus),
        BuiltinKind::TmuxPane => make_dynamic_tool::<TmuxPaneTool>(def, cwd, max_bytes, bus),
        BuiltinKind::TmuxLayout => make_dynamic_tool::<TmuxLayoutTool>(def, cwd, max_bytes, bus),
        BuiltinKind::AstQuery => make_dynamic_tool::<AstQueryTool>(def, cwd, max_bytes, bus),
        BuiltinKind::AstNodes => make_dynamic_tool::<AstNodesTool>(def, cwd, max_bytes, bus),
        BuiltinKind::AstRefs => make_dynamic_tool::<AstRefsTool>(def, cwd, max_bytes, bus),
        BuiltinKind::AstTree => make_dynamic_tool::<AstTreeTool>(def, cwd, max_bytes, bus),
        _ => return,
    };
    tool_server.add_dynamic_tool(tool).await;
}

#[cfg(test)]
#[path = "builtin_tool_test.rs"]
mod tests;
