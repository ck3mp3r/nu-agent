use crate::tools::mcp::filter::matches_patterns;

#[derive(Debug, Clone, PartialEq)]
pub struct McpToolDefinition {
    pub server: String,
    pub name: String,
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
