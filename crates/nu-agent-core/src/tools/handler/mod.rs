mod authz_gate;
pub mod builtin_fs;
mod conversion;
mod dispatch;
pub mod messaging;
pub mod pre_authorize;
mod result;
pub mod spawn_agent;
mod types;

#[cfg(test)]
#[path = "spawn_agent_test.rs"]
mod spawn_agent_test;

pub use conversion::{json_to_nu_value, nu_value_to_json};
pub use dispatch::llm_visible_tool_definitions;
pub use pre_authorize::PreAuthorizeOutput;
pub use result::build_direct_tool_display;
pub use types::{
    McpToolRegistry, ToolAuthorizationContext, ToolErrorKind, ToolFailureOutcome,
    ToolHandlerContext, ToolSource,
};

// Export authz_gate types for permission_bridge
pub use authz_gate::{AuthorizationFlowContext, enforce_authorization_for_tool_call};

/// Returns true for filesystem-mutating builtin tools (`edit`, `patch`).
/// These are classified as `ToolSource::BuiltinFs` and go through the full
/// permission flow — they are NOT auto-approved despite being built-in.
pub fn is_builtin_fs_tool_name(tool_name: &str) -> bool {
    matches!(tool_name, "edit" | "patch")
}

pub fn is_builtin_tool_name(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "read"
            | "edit"
            | "patch"
            | "skill"
            | "spawn_agent"
            | "terminate_agent"
            | "send_message"
            | "list_agents"
    )
}
