use crate::tools::mcp::filter::matches_patterns;

#[derive(Debug, Clone, PartialEq)]
pub struct McpToolDefinition {
    pub server: String,
    /// Exposed/callable tool name in `<server_key>::<raw_tool_name>` format.
    pub name: String,
    /// Raw server-advertised tool name, retained for MCP call mapping.
    pub raw_name: String,
    pub description: Option<String>,
    pub parameters: Option<serde_json::Value>,
}

pub fn filter_tools(tools: &[McpToolDefinition], patterns: &[String]) -> Vec<McpToolDefinition> {
    tools
        .iter()
        .filter(|tool| matches_patterns(&tool.name, patterns))
        .cloned()
        .collect()
}

#[cfg(test)]
mod test;
