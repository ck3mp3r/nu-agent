//! Bridges the PermissionResolver trait to the existing authorization system.

use nu_plugin::EngineInterface;
use rig::completion::message::{ToolCall, ToolFunction};
use serde_json::Value as JsonValue;

use super::types::PermissionDecision;
use crate::agent::tools::authz::{
    AskApprovalHook, PermissionEventSink, PermissionsConfig, SessionGrantCache,
};
use crate::agent::tools::handler::{
    McpToolRegistry, ToolSource, is_builtin_fs_tool_name, is_builtin_tool_name,
};
use crate::tools::closure::ClosureRegistry;

/// Resolve the source of a tool by checking the closure and MCP registries.
pub fn resolve_tool_source(
    name: &str,
    closures: &ClosureRegistry,
    mcp: &McpToolRegistry,
) -> ToolSource {
    if closures.get(name).is_some() {
        ToolSource::Closure
    } else if is_builtin_fs_tool_name(name) {
        ToolSource::BuiltinFs
    } else if is_builtin_tool_name(name) {
        ToolSource::Builtin
    } else if mcp.contains(name) {
        ToolSource::Mcp
    } else {
        ToolSource::Unknown
    }
}

/// Bridges the PermissionResolver trait to the existing authorization system.
///
/// This adapter wraps the full authorization flow (pre-authorization, session grants,
/// ask prompts, and enforcement) and exposes it via the simplified PermissionResolver trait.
pub struct AuthzPermissionResolver<'a, H>
where
    H: AskApprovalHook,
{
    pub permissions: &'a PermissionsConfig,
    pub grant_cache: &'a mut SessionGrantCache,
    pub ask_hook: &'a mut H,
    pub engine: &'a EngineInterface,
    pub closure_registry: &'a ClosureRegistry,
    pub mcp_registry: &'a McpToolRegistry,
}

impl<H> super::driver::PermissionResolver for AuthzPermissionResolver<'_, H>
where
    H: AskApprovalHook,
{
    fn resolve<S: PermissionEventSink>(
        &mut self,
        tool_name: &str,
        arguments: &str,
        tool_call_id: Option<String>,
        sink: &mut S,
    ) -> PermissionDecision {
        // 1. Parse arguments JSON (or use empty object if invalid)
        let args_json: JsonValue =
            serde_json::from_str(arguments).unwrap_or(JsonValue::Object(serde_json::Map::new()));

        // 2. Build ToolCall with real or fallback ID
        let call_id = tool_call_id.unwrap_or_else(|| "synthetic".to_string());
        let tool_call = ToolCall::new(call_id, ToolFunction::new(tool_name.to_string(), args_json));

        // 3. Determine tool source using the registries
        let source = resolve_tool_source(tool_name, self.closure_registry, self.mcp_registry);

        // 4. Pre-authorize (provides context for ask prompts, e.g., edit previews)
        let pre_authorize_output =
            crate::agent::tools::handler::pre_authorize::pre_authorize_tool_call(
                &tool_call,
                source.clone(),
                self.engine,
            );

        let flow_context = crate::agent::tools::handler::AuthorizationFlowContext {
            ask_context: pre_authorize_output.ask_context.clone(),
        };

        // 5. Enforce authorization (includes session grant checks and ask prompts)
        let denied_details = crate::agent::tools::handler::enforce_authorization_for_tool_call(
            &tool_call,
            source,
            self.permissions,
            self.grant_cache,
            &flow_context,
            self.ask_hook,
            sink,
        );

        // 6. Map to PermissionDecision
        if denied_details.is_some() {
            PermissionDecision::Deny
        } else {
            PermissionDecision::Allow
        }
    }
}

#[cfg(test)]
#[path = "permission_bridge_test.rs"]
mod permission_bridge_test;
