use crate::protocol::contracts::McpUsabilityState;

/// Runtime capability for MCP server management.
pub trait HasMcpManagement {
    fn set_mcp_server_enabled(
        &mut self,
        name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String>;

    fn llm_visible_mcp_tool_count(&self) -> usize;

    fn llm_visible_mcp_tool_count_for_server(&self, server_name: &str) -> usize;

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)>;
}
