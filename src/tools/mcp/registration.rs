use crate::tools::mcp::client::{McpToolDefinition, filter_tools};

pub fn registerable_tools(
    runtime_discovered: &[McpToolDefinition],
    cli_patterns: &[String],
) -> Vec<McpToolDefinition> {
    filter_tools(runtime_discovered, cli_patterns)
}

#[cfg(test)]
#[path = "registration_tests.rs"]
mod registration_tests;
