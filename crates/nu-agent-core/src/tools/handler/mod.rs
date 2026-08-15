use nu_plugin::EngineInterface;

mod authz_gate;
pub mod builtin_kinds;
pub mod builtin_tool;
mod conversion;
mod dispatch;
pub mod edit;
pub mod glob;
pub mod grep;
pub mod http;
pub mod nu;
pub mod patch;
pub mod pre_authorize;
pub mod read;
mod result;
pub mod skill;
pub mod tmux_common;
pub mod tmux_layout;
pub mod tmux_pane;
pub mod tmux_session;
pub mod tmux_window;
pub mod tree_sitter;
mod types;

#[cfg(test)]
mod http_test;

pub use conversion::{json_to_nu_value, nu_value_to_json};
pub use dispatch::llm_visible_tool_definitions;
pub use pre_authorize::PreAuthorizeOutput;
pub use result::build_direct_tool_display;
pub use types::{
    McpToolRegistry, ToolAuthorizationContext, ToolErrorKind, ToolFailureOutcome,
    ToolHandlerContext, ToolHandlerError, ToolSource,
};

// Export authz_gate types for permission resolvers
pub use authz_gate::{AuthorizationFlowContext, enforce_authorization_for_tool_call};

pub fn is_builtin_tool_name(tool_name: &str) -> bool {
    tool_name.parse::<builtin_kinds::BuiltinKind>().is_ok()
}

pub(crate) fn resolve_fs_path_for_cwd(path: &str, cwd: &std::path::Path) -> std::path::PathBuf {
    let raw = std::path::Path::new(path);
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        cwd.join(raw)
    }
}

pub(crate) fn resolve_fs_path(
    path: &str,
    engine: &EngineInterface,
) -> Result<std::path::PathBuf, ToolHandlerError> {
    let cwd = engine.get_current_dir().map_err(|e| ToolHandlerError {
        kind: ToolErrorKind::Runtime,
        message: format!("Unable to resolve current working directory: {e}"),
        details: None,
    })?;
    Ok(resolve_fs_path_for_cwd(path, std::path::Path::new(&cwd)))
}
