mod authz_gate;
pub(crate) mod builtin_fs;
mod conversion;
mod dispatch;
pub(crate) mod messaging;
pub(crate) mod pre_authorize;
mod result;
pub(crate) mod spawn_agent;
mod types;

#[cfg(test)]
#[path = "spawn_agent_test.rs"]
mod spawn_agent_test;

pub use conversion::{json_to_nu_value, nu_value_to_json};
pub(crate) use dispatch::llm_visible_tool_definitions;
pub use pre_authorize::PreAuthorizeOutput;
pub(crate) use result::build_direct_tool_display;
pub use types::{
    McpToolRegistry, ToolAuthorizationContext, ToolErrorKind, ToolFailureOutcome,
    ToolHandlerContext, ToolSource,
};

// Export authz_gate types for permission_bridge
pub(crate) use authz_gate::{AuthorizationFlowContext, enforce_authorization_for_tool_call};

pub(crate) fn is_builtin_fs_tool_name(tool_name: &str) -> bool {
    matches!(tool_name, "edit" | "patch")
}

pub(crate) fn is_builtin_tool_name(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "read" | "edit" | "patch" | "skill" | "spawn_agent" | "send_message" | "list_agents"
    )
}
