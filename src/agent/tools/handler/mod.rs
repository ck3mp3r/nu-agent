mod authz_gate;
pub(crate) mod builtin_fs;
mod conversion;
mod dispatch;
pub(crate) mod pre_authorize;
mod result;
mod types;

pub use conversion::{json_to_nu_value, nu_value_to_json};
pub use dispatch::handle_tool_calls;
pub(crate) use dispatch::llm_visible_tool_definitions;
pub use pre_authorize::PreAuthorizeOutput;
pub(crate) use result::{
    build_authorization_denied_result, build_direct_tool_display, build_failure_result,
    classify_validation_error_message,
};
pub use types::{
    McpToolRegistry, ToolAuthorizationContext, ToolCallResult, ToolErrorKind, ToolFailureOutcome,
    ToolHandlerContext, ToolSource,
};

pub(crate) fn is_builtin_fs_tool_name(tool_name: &str) -> bool {
    matches!(tool_name, "read" | "edit" | "patch" | "skill")
}

#[cfg(test)]
mod test;
