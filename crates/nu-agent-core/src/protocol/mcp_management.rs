use crate::protocol::contracts::McpUsabilityState;

/// Runtime capability for MCP server management.
pub trait McpManagement {
    fn set_mcp_server_enabled(
        &mut self,
        _name: &str,
        enabled: bool,
    ) -> impl std::future::Future<Output = Result<McpUsabilityState, String>> + Send {
        async move {
            Ok(if enabled {
                McpUsabilityState::Enabled
            } else {
                McpUsabilityState::Disabled
            })
        }
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        0
    }

    fn llm_visible_mcp_tool_count_for_server(&self, _server_name: &str) -> usize {
        0
    }

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
        Vec::new()
    }
}
