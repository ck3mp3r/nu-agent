use crate::tools::mcp::client::{McpToolDefinition, filter_tools};

pub fn registerable_tools(
    runtime_discovered: &[McpToolDefinition],
    cli_patterns: &[String],
) -> Vec<McpToolDefinition> {
    log::trace!("registerable_tools: discovered={}, patterns={cli_patterns:?}", runtime_discovered.len());
    filter_tools(runtime_discovered, cli_patterns)
}

#[cfg(test)]
#[path = "registration_test.rs"]
mod registration_test;
