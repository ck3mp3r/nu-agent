use super::McpToolRegistry;
use crate::tools::authz::PermissionsConfig;
use crate::types::ToolDefinition;

pub fn llm_visible_tool_definitions(
    tool_definitions: &[ToolDefinition],
    mcp_registry: &McpToolRegistry,
    permissions: &PermissionsConfig,
) -> Vec<ToolDefinition> {
    let result: Vec<_> = tool_definitions
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
        .collect();
    log::debug!(
        "llm_visible_tool_definitions: total={} visible={}",
        tool_definitions.len(),
        result.len()
    );
    result
}

#[cfg(test)]
#[path = "dispatch_test.rs"]
mod dispatch_test;
