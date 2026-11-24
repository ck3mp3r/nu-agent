//! The server set and the exposed tool vocabulary: which servers connect, the
//! lifecycle projection the surface shows, and the names an MCP tool takes.

use super::McpServerLifecycle;
use crate::tools::mcp::MCP_TOOL_NAMESPACE_DELIMITER;
use crate::tools::mcp::config::McpServerConfig;

pub(crate) fn select_enabled_servers(servers: &[McpServerConfig]) -> Vec<&McpServerConfig> {
    servers.iter().filter(|server| server.enabled).collect()
}

pub(crate) fn project_server_lifecycle(
    configured_servers: &[McpServerConfig],
    connected_servers: &std::collections::BTreeSet<String>,
) -> Vec<McpServerLifecycle> {
    let mut projection: Vec<McpServerLifecycle> = configured_servers
        .iter()
        .map(|server| McpServerLifecycle {
            name: server.name.clone(),
            configured: true,
            enabled: server.enabled,
            connected: connected_servers.contains(&server.name),
            visible_tool_count: 0,
            failure_reason: None,
        })
        .collect();

    projection.sort_by(|a, b| a.name.cmp(&b.name));
    projection
}

pub(crate) fn compose_exposed_tool_name(server_key: &str, raw_tool_name: &str) -> String {
    format!("{server_key}{MCP_TOOL_NAMESPACE_DELIMITER}{raw_tool_name}")
}

pub(crate) fn validate_raw_tool_name(server_name: &str, raw_tool_name: &str) -> Result<(), String> {
    if raw_tool_name.contains(MCP_TOOL_NAMESPACE_DELIMITER) {
        return Err(format!(
            "MCP server '{}' advertised tool '{}' containing reserved delimiter '{}'",
            server_name, raw_tool_name, MCP_TOOL_NAMESPACE_DELIMITER
        ));
    }

    Ok(())
}

pub(crate) fn register_exposed_name(
    exposed_name_owner: &mut std::collections::HashMap<String, String>,
    tool_name: &str,
    server_name: &str,
) -> Result<(), String> {
    if let Some(existing_owner) =
        exposed_name_owner.insert(tool_name.to_string(), server_name.to_string())
    {
        return Err(format!(
            "duplicate exposed MCP tool name '{}' discovered for servers '{}' and '{}'",
            tool_name, existing_owner, server_name
        ));
    }

    Ok(())
}
