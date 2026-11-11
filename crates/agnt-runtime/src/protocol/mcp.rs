//! The MCP management contract: whether a server is usable, and the runtime
//! capability that toggles one.

/// The usability of one MCP server, as the runtime reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpUsabilityState {
    Enabled,
    Disabled,
    Failed,
}

impl From<McpUsabilityState> for agnt_kernel::bus::McpServerUsability {
    fn from(state: McpUsabilityState) -> Self {
        match state {
            McpUsabilityState::Enabled => Self::Enabled,
            McpUsabilityState::Disabled => Self::Disabled,
            McpUsabilityState::Failed => Self::Failed,
        }
    }
}

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
