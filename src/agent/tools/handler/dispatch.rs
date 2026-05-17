use super::McpToolRegistry;

pub(crate) fn llm_visible_tool_definitions(
    tool_definitions: &[rig::completion::ToolDefinition],
    mcp_registry: &McpToolRegistry,
) -> Vec<rig::completion::ToolDefinition> {
    tool_definitions
        .iter()
        .filter(|tool| {
            if mcp_registry.is_registered(tool.name.as_str()) {
                mcp_registry.contains(tool.name.as_str())
            } else {
                true
            }
        })
        .cloned()
        .collect()
}
