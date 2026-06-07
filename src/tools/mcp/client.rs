use crate::tools::mcp::filter::matches_patterns;

#[derive(Debug, Clone, PartialEq)]
pub struct McpToolDefinition {
    pub server: String,
    /// Exposed/callable tool name in `<server_key><delimiter><raw_tool_name>` format.
    pub name: String,
    /// Raw server-advertised tool name, retained for MCP call mapping.
    pub raw_name: String,
    pub description: Option<String>,
    pub parameters: Option<serde_json::Value>,
}

impl McpToolDefinition {
    /// Create a test-friendly McpToolDefinition with minimal fields
    #[cfg(test)]
    pub fn test_tool(server: impl Into<String>, name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            server: server.into(),
            name: name.clone(),
            raw_name: name,
            description: None,
            parameters: None,
        }
    }

    /// Create a test McpToolDefinition with custom raw_name
    #[cfg(test)]
    pub fn test_tool_with_raw(
        server: impl Into<String>,
        name: impl Into<String>,
        raw_name: impl Into<String>,
    ) -> Self {
        Self {
            server: server.into(),
            name: name.into(),
            raw_name: raw_name.into(),
            description: None,
            parameters: None,
        }
    }
}

pub fn filter_tools(tools: &[McpToolDefinition], patterns: &[String]) -> Vec<McpToolDefinition> {
    log::debug!(
        "filter_tools: input_count={}, pattern_count={}",
        tools.len(),
        patterns.len()
    );

    let result: Vec<McpToolDefinition> = tools
        .iter()
        .filter(|tool| matches_patterns(&tool.name, patterns))
        .cloned()
        .collect();

    log::debug!("filter_tools: output_count={}", result.len());
    result
}

#[cfg(test)]
#[path = "client_test.rs"]
mod client_test;
