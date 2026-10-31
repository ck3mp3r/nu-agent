use crate::tools::mcp::{
    client::McpToolDefinition,
    config::{McpAuthConfig, McpServerConfig, McpTransportType},
};
use crate::types::{EmptyToolName, ToolDefinition, ToolName};

pub fn mcp_tool(server: &str, name: &str, raw_name: &str) -> McpToolDefinition {
    McpToolDefinition {
        server: server.to_string(),
        name: name.to_string(),
        raw_name: raw_name.to_string(),
        description: Some(format!("{server}:{raw_name}")),
        parameters: Some(serde_json::json!({"type":"object"})),
    }
}

pub fn tool_definition_named(name: &str) -> Result<ToolDefinition, EmptyToolName> {
    Ok(ToolDefinition {
        annotations: crate::types::ToolAnnotations::default(),
        name: ToolName::new(name)?,
        description: format!("tool {name}"),
        parameters: serde_json::json!({"type":"object"}),
    })
}

pub fn mcp_server_config(name: &str, enabled: bool) -> McpServerConfig {
    McpServerConfig {
        name: name.to_string(),
        transport: McpTransportType::Http,
        enabled,
        url: Some("http://localhost:7777/mcp".to_string()),
        headers: std::collections::HashMap::new(),
        auth: McpAuthConfig::None,
        command: None,
        cwd: None,
        args: Vec::new(),
        env: std::collections::HashMap::new(),
    }
}
