use super::McpToolRegistry;
use crate::agent::tools::authz::PermissionsConfig;

pub(crate) fn llm_visible_tool_definitions(
    tool_definitions: &[rig::completion::ToolDefinition],
    mcp_registry: &McpToolRegistry,
    permissions: &PermissionsConfig,
) -> Vec<rig::completion::ToolDefinition> {
    tool_definitions
        .iter()
        .filter(|tool| {
            if !permissions.is_tool_visible(&tool.name) {
                return false;
            }
            if mcp_registry.is_registered(tool.name.as_str()) {
                mcp_registry.contains(tool.name.as_str())
            } else {
                true
            }
        })
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "dispatch_test.rs"]
mod dispatch_test;
