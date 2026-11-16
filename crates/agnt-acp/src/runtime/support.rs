//! Builder support: server merging, tool definitions, and config-error mapping.

use agnt_runtime::tools::mcp::client::McpToolDefinition;
use agnt_runtime::tools::mcp::config::McpServerConfig;
use agnt_runtime::types::{EmptyToolName, ToolAnnotations, ToolDefinition, ToolName};

use crate::error::AcpError;

/// Merge config.toml MCP servers with the ACP client's servers.
///
/// A client server replaces a config.toml server with the same name; servers
/// with names unique to either source are all kept.
pub(super) fn merge_mcp_servers(
    config_servers: Vec<McpServerConfig>,
    client_servers: Vec<McpServerConfig>,
) -> Vec<McpServerConfig> {
    let mut merged = config_servers;
    for client in client_servers {
        match merged.iter_mut().find(|server| server.name == client.name) {
            Some(existing) => *existing = client,
            None => merged.push(client),
        }
    }
    merged
}

pub(super) fn mcp_tool_definition(
    tool: &McpToolDefinition,
) -> core::result::Result<ToolDefinition, EmptyToolName> {
    Ok(ToolDefinition {
        annotations: ToolAnnotations::default(),
        name: ToolName::new(tool.name.clone())?,
        description: tool
            .description
            .clone()
            .unwrap_or_else(|| format!("MCP tool from server '{}'", tool.server)),
        parameters: tool.parameters.clone().unwrap_or_else(|| {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "args": {
                        "type": "array",
                        "items": {}
                    }
                },
                "required": ["args"]
            })
        }),
    })
}

/// Map a shared config-load error to an ACP error.
pub(super) fn map_config_load_error(err: agnt_runtime::config::ConfigLoadError) -> AcpError {
    use agnt_runtime::config::ConfigLoadError;
    match err {
        ConfigLoadError::NotFound(path) => {
            AcpError::Config(format!("config not found at {}", path.display()))
        }
        ConfigLoadError::Toml(e) => AcpError::Config(format!("failed to load config: {e}")),
        ConfigLoadError::UnknownRole(role) => {
            AcpError::Config(format!("unknown model role: '{role}'"))
        }
        ConfigLoadError::NoDefaultModel => AcpError::Config(
            "no model configured: add a [models.default] block to config.toml".to_string(),
        ),
        ConfigLoadError::Resolve(msg) => {
            AcpError::Config(format!("failed to resolve model: {msg}"))
        }
        ConfigLoadError::Vault(e) => AcpError::Config(e.to_string()),
        ConfigLoadError::Paths(e) => {
            AcpError::Config(format!("failed to resolve application paths: {e}"))
        }
    }
}
