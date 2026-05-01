use std::collections::HashMap;

use crate::tools::mcp::config::{McpServerConfig, McpTransportType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpTransportSpec {
    Stdio {
        command: String,
        cwd: Option<String>,
        args: Vec<String>,
        env: HashMap<String, String>,
    },
    Sse {
        url: String,
        headers: HashMap<String, String>,
    },
    Http {
        url: String,
        headers: HashMap<String, String>,
    },
}

pub fn build_transport_spec(server: &McpServerConfig) -> Result<McpTransportSpec, String> {
    match server.transport {
        McpTransportType::Stdio => Ok(McpTransportSpec::Stdio {
            command: server.command.clone().ok_or_else(|| {
                format!(
                    "MCP server '{}' with transport 'stdio' requires command",
                    server.name
                )
            })?,
            cwd: server.cwd.clone(),
            args: server.args.clone(),
            env: server.env.clone(),
        }),
        McpTransportType::Sse => Ok(McpTransportSpec::Sse {
            url: server.url.clone().ok_or_else(|| {
                format!(
                    "MCP server '{}' with transport 'sse' requires url",
                    server.name
                )
            })?,
            headers: server.headers.clone(),
        }),
        McpTransportType::Http | McpTransportType::StreamableHttp => Ok(McpTransportSpec::Http {
            url: server.url.clone().ok_or_else(|| {
                format!(
                    "MCP server '{}' with transport 'http' requires url",
                    server.name
                )
            })?,
            headers: server.headers.clone(),
        }),
    }
}

#[cfg(test)]
mod test;
