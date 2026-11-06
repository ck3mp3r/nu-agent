use std::path::{Path, PathBuf};
use std::str::FromStr;

use super::edit::EditTool;
use super::glob::GlobTool;
use super::grep::GrepTool;
use super::http::HttpTool;
use super::nu::NuTool;
use super::read::ReadTool;
use super::skill::SkillTool;
use super::tmux_layout::TmuxLayoutTool;
use super::tmux_pane::TmuxPaneTool;
use super::tmux_session::TmuxSessionTool;
use super::tmux_window::TmuxWindowTool;
use super::tree_sitter::{AstNodesTool, AstQueryTool, AstRefsTool, AstTreeTool};
use super::{ToolHandlerError, execution_error_for_kind};
use crate::tools::limits::truncate_tool_output;
use crate::types::ToolDefinition;
use agnt_kernel::BuiltinKind;
use agnt_kernel::bus::CancelTx;
use rig::tool::server::ToolServerHandle;
use rig::tool::{DynamicTool, ToolExecutionError, ToolOutput};
use serde_json::Value as JsonValue;

pub trait BuiltinTool: Sized {
    const NAME: &'static str;
    fn execute(
        args: &JsonValue,
        cwd: &Path,
        cancel: &CancelTx,
    ) -> impl std::future::Future<Output = Result<JsonValue, ToolHandlerError>> + Send;
}

/// Tools whose pre-execution preview needs IO nobody else can do. `EditTool`
/// returns the proposed diff; every other tool's preview is derived from its
/// arguments by the surface, so it keeps the default (`None`).
pub trait Previewable: BuiltinTool {
    fn preview(args: &JsonValue, cwd: &Path) -> Option<String> {
        let _ = (args, cwd);
        None
    }
}

pub fn make_dynamic_tool<T: BuiltinTool>(
    def: ToolDefinition,
    cwd: PathBuf,
    max_bytes: usize,
    cancel: &CancelTx,
) -> DynamicTool {
    let name = def.name.clone();
    let description = def.description.clone();
    let parameters = def.parameters.clone();
    let cancel = cancel.clone();
    DynamicTool::new(name, description, parameters, move |args| {
        let cwd = cwd.clone();
        let cancel = cancel.clone();
        Box::pin(async move {
            let result = match T::execute(&args, &cwd, &cancel).await {
                Ok(result) => result,
                Err(e) => {
                    // Handler failures keep the failure shape: the details
                    // payload is serialized once, truncated like a success
                    // payload, embedded in the message, and attached as the
                    // model output so the model sees the same JSON it saw
                    // before the payload became an error. The handler's
                    // error kind selects the rig error kind so downstream
                    // retry and refusal policy sees the real classification.
                    let kind = e.kind;
                    let message = e.message.clone();
                    let mapped = match e.details {
                        Some(details) => {
                            let json_str = serde_json::to_string(&details).map_err(|err| {
                                ToolExecutionError::other(format!(
                                    "JSON serialization failed: {err}"
                                ))
                            })?;
                            let truncated = truncate_tool_output(json_str, max_bytes);
                            execution_error_for_kind(kind, format!("{message}: {truncated}"))
                                .with_model_output(ToolOutput::text(truncated))
                        }
                        None => execution_error_for_kind(kind, format!("{message}: no details")),
                    };
                    return Err(mapped);
                }
            };
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
    cancel: &CancelTx,
    tool_server: &ToolServerHandle,
) {
    let kind = match BuiltinKind::from_str(&def.name) {
        Ok(k) => k,
        Err(_) => return,
    };
    let tool = match kind {
        BuiltinKind::Read => make_dynamic_tool::<ReadTool>(def, cwd, max_bytes, cancel),
        BuiltinKind::Edit => make_dynamic_tool::<EditTool>(def, cwd, max_bytes, cancel),
        BuiltinKind::Skill => make_dynamic_tool::<SkillTool>(def, cwd, max_bytes, cancel),
        BuiltinKind::Grep => make_dynamic_tool::<GrepTool>(def, cwd, max_bytes, cancel),
        BuiltinKind::Glob => make_dynamic_tool::<GlobTool>(def, cwd, max_bytes, cancel),
        BuiltinKind::Http => make_dynamic_tool::<HttpTool>(def, cwd, max_bytes, cancel),
        BuiltinKind::Nu => make_dynamic_tool::<NuTool>(def, cwd, max_bytes, cancel),
        BuiltinKind::TmuxSession => {
            make_dynamic_tool::<TmuxSessionTool>(def, cwd, max_bytes, cancel)
        }
        BuiltinKind::TmuxWindow => make_dynamic_tool::<TmuxWindowTool>(def, cwd, max_bytes, cancel),
        BuiltinKind::TmuxPane => make_dynamic_tool::<TmuxPaneTool>(def, cwd, max_bytes, cancel),
        BuiltinKind::TmuxLayout => make_dynamic_tool::<TmuxLayoutTool>(def, cwd, max_bytes, cancel),
        BuiltinKind::AstQuery => make_dynamic_tool::<AstQueryTool>(def, cwd, max_bytes, cancel),
        BuiltinKind::AstNodes => make_dynamic_tool::<AstNodesTool>(def, cwd, max_bytes, cancel),
        BuiltinKind::AstRefs => make_dynamic_tool::<AstRefsTool>(def, cwd, max_bytes, cancel),
        BuiltinKind::AstTree => make_dynamic_tool::<AstTreeTool>(def, cwd, max_bytes, cancel),
        _ => return,
    };
    tool_server.add_dynamic_tool(tool);
}

#[cfg(test)]
#[path = "../../../test/tools/handler/builtin_tool.rs"]
mod tests;
