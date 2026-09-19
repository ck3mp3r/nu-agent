use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};
use std::str::FromStr;

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
use super::{ToolErrorKind, ToolHandlerError};
use crate::bus::Bus;
use crate::protocol::tool_args::CallLineRender;
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

    /// Render the call line for this tool's arguments. The default is the
    /// generic JSON summary; tools override it to show a tailored summary
    /// or a code block.
    fn call_line_render(arguments: &str) -> CallLineRender {
        CallLineRender::generic_json_summary(arguments)
    }
}

/// A tool's call-line render function.
pub type RenderFn = fn(&str) -> CallLineRender;

/// Maps tool names to their call-line render functions. Populated at
/// registration time, where the concrete tool type is known, and consulted
/// when a tool call starts to build the transcript call line.
#[derive(Clone, Default)]
pub struct ToolRenderRegistry {
    fns: std::collections::HashMap<String, RenderFn>,
}

impl ToolRenderRegistry {
    /// Register `f` as the render function for `name`.
    pub fn register(&mut self, name: &str, f: RenderFn) {
        self.fns.insert(name.to_string(), f);
    }

    /// Render the call line for `name` with `arguments`. Unknown names fall
    /// back to the generic JSON summary.
    pub fn render(&self, name: &str, arguments: &str) -> CallLineRender {
        match self.fns.get(name) {
            Some(f) => f(arguments),
            None => CallLineRender::generic_json_summary(arguments),
        }
    }
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
            let result = match T::execute(&args, &cwd, &bus).await {
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
    bus: Bus,
    tool_server: &ToolServerHandle,
    render_registry: &mut ToolRenderRegistry,
) {
    let kind = match BuiltinKind::from_str(&def.name) {
        Ok(k) => k,
        Err(_) => return,
    };
    let tool = match kind {
        BuiltinKind::Read => {
            render_registry.register(&def.name, ReadTool::call_line_render);
            make_dynamic_tool::<ReadTool>(def, cwd, max_bytes, bus)
        }
        BuiltinKind::Edit => {
            render_registry.register(&def.name, EditTool::call_line_render);
            make_dynamic_tool::<EditTool>(def, cwd, max_bytes, bus)
        }
        BuiltinKind::Patch => {
            render_registry.register(&def.name, PatchTool::call_line_render);
            make_dynamic_tool::<PatchTool>(def, cwd, max_bytes, bus)
        }
        BuiltinKind::Skill => {
            render_registry.register(&def.name, SkillTool::call_line_render);
            make_dynamic_tool::<SkillTool>(def, cwd, max_bytes, bus)
        }
        BuiltinKind::Grep => {
            render_registry.register(&def.name, GrepTool::call_line_render);
            make_dynamic_tool::<GrepTool>(def, cwd, max_bytes, bus)
        }
        BuiltinKind::Glob => {
            render_registry.register(&def.name, GlobTool::call_line_render);
            make_dynamic_tool::<GlobTool>(def, cwd, max_bytes, bus)
        }
        BuiltinKind::Http => {
            render_registry.register(&def.name, HttpTool::call_line_render);
            make_dynamic_tool::<HttpTool>(def, cwd, max_bytes, bus)
        }
        BuiltinKind::Nu => {
            render_registry.register(&def.name, NuTool::call_line_render);
            make_dynamic_tool::<NuTool>(def, cwd, max_bytes, bus)
        }
        BuiltinKind::TmuxSession => {
            render_registry.register(&def.name, TmuxSessionTool::call_line_render);
            make_dynamic_tool::<TmuxSessionTool>(def, cwd, max_bytes, bus)
        }
        BuiltinKind::TmuxWindow => {
            render_registry.register(&def.name, TmuxWindowTool::call_line_render);
            make_dynamic_tool::<TmuxWindowTool>(def, cwd, max_bytes, bus)
        }
        BuiltinKind::TmuxPane => {
            render_registry.register(&def.name, TmuxPaneTool::call_line_render);
            make_dynamic_tool::<TmuxPaneTool>(def, cwd, max_bytes, bus)
        }
        BuiltinKind::TmuxLayout => {
            render_registry.register(&def.name, TmuxLayoutTool::call_line_render);
            make_dynamic_tool::<TmuxLayoutTool>(def, cwd, max_bytes, bus)
        }
        BuiltinKind::AstQuery => {
            render_registry.register(&def.name, AstQueryTool::call_line_render);
            make_dynamic_tool::<AstQueryTool>(def, cwd, max_bytes, bus)
        }
        BuiltinKind::AstNodes => {
            render_registry.register(&def.name, AstNodesTool::call_line_render);
            make_dynamic_tool::<AstNodesTool>(def, cwd, max_bytes, bus)
        }
        BuiltinKind::AstRefs => {
            render_registry.register(&def.name, AstRefsTool::call_line_render);
            make_dynamic_tool::<AstRefsTool>(def, cwd, max_bytes, bus)
        }
        BuiltinKind::AstTree => {
            render_registry.register(&def.name, AstTreeTool::call_line_render);
            make_dynamic_tool::<AstTreeTool>(def, cwd, max_bytes, bus)
        }
        _ => return,
    };
    tool_server.add_dynamic_tool(tool).await;
}

// region:    --- Support

/// Map a handler error kind to the matching rig `ToolExecutionError`
/// constructor so downstream retry and refusal policy sees the real
/// classification instead of a flat provider failure.
fn execution_error_for_kind(kind: ToolErrorKind, message: String) -> ToolExecutionError {
    match kind {
        ToolErrorKind::Validation => ToolExecutionError::invalid_args(message),
        ToolErrorKind::Timeout => ToolExecutionError::timeout(message),
        ToolErrorKind::Authorization => ToolExecutionError::permission_denied(message),
        ToolErrorKind::Transport => ToolExecutionError::network(message),
        ToolErrorKind::Runtime | ToolErrorKind::Unknown => ToolExecutionError::other(message),
    }
}

// endregion: --- Support

#[cfg(test)]
#[path = "builtin_tool_test.rs"]
mod tests;
