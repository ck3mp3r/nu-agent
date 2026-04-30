use http::{HeaderName, HeaderValue};
use rig::tool::server::ToolServer;

use crate::tools::mcp::{
    client::McpToolDefinition,
    config::McpServerConfig,
    transport::{McpTransportSpec, build_transport_spec},
};

pub struct McpRuntime {
    tool_server_handle: rig::tool::server::ToolServerHandle,
    sessions: Vec<McpSessionHandle>,
    discovered_tools: Vec<McpToolDefinition>,
}

enum McpSessionHandle {
    #[allow(dead_code)]
    Rmcp(
        rmcp::service::RunningService<rmcp::service::RoleClient, rig::tool::rmcp::McpClientHandler>,
    ),
}

fn build_http_transport_config(
    spec: &McpTransportSpec,
) -> Result<rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig, String> {
    let (url, headers, allow_stateless) = match spec {
        McpTransportSpec::Sse { url, headers } => (url.clone(), headers.clone(), true),
        McpTransportSpec::Http { url, headers } => (url.clone(), headers.clone(), false),
        McpTransportSpec::Stdio { .. } => {
            return Err("invalid transport spec for HTTP config".to_string());
        }
    };

    let mut custom_headers = std::collections::HashMap::new();
    for (name, value) in headers {
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| format!("invalid MCP header name '{}': {e}", name))?;
        let header_value = HeaderValue::from_str(&value)
            .map_err(|e| format!("invalid MCP header value for '{}': {e}", name))?;
        custom_headers.insert(header_name, header_value);
    }

    let mut config =
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(url)
            .custom_headers(custom_headers);
    config.allow_stateless = allow_stateless;
    Ok(config)
}

impl McpRuntime {
    pub fn tool_server_handle(&self) -> rig::tool::server::ToolServerHandle {
        self.tool_server_handle.clone()
    }

    pub fn has_sessions(&self) -> bool {
        !self.sessions.is_empty()
    }

    pub fn discovered_tools(&self) -> &[McpToolDefinition] {
        &self.discovered_tools
    }
}

pub async fn connect_servers(servers: &[McpServerConfig]) -> Result<McpRuntime, String> {
    let tool_server_handle = ToolServer::new().run();

    let mut sessions = Vec::new();
    let mut discovered_tools = Vec::new();
    for server in servers {
        let spec = build_transport_spec(server)?;
        let (service, server_tools) =
            connect_server(&tool_server_handle, &server.name, spec).await?;
        discovered_tools.extend(server_tools);
        sessions.push(McpSessionHandle::Rmcp(service));
    }

    Ok(McpRuntime {
        tool_server_handle,
        sessions,
        discovered_tools,
    })
}

async fn connect_server(
    tool_server_handle: &rig::tool::server::ToolServerHandle,
    server_name: &str,
    spec: McpTransportSpec,
) -> Result<
    (
        rmcp::service::RunningService<rmcp::service::RoleClient, rig::tool::rmcp::McpClientHandler>,
        Vec<McpToolDefinition>,
    ),
    String,
> {
    let client_info = rmcp::model::ClientInfo::new(
        rmcp::model::ClientCapabilities::default(),
        rmcp::model::Implementation::new("nu-agent", env!("CARGO_PKG_VERSION")),
    );
    let handler = rig::tool::rmcp::McpClientHandler::new(client_info, tool_server_handle.clone());

    match spec {
        McpTransportSpec::Stdio { command, args, env } => {
            let mut cmd = tokio::process::Command::new(command);
            for arg in args {
                cmd.arg(arg);
            }
            for (k, v) in env {
                cmd.env(k, v);
            }
            let transport = rmcp::transport::TokioChildProcess::new(cmd)
                .map_err(|e| format!("failed to build stdio transport: {e}"))?;

            let service = handler
                .connect(transport)
                .await
                .map_err(|e| format!("failed to connect stdio MCP server: {e}"))?;

            let discovered_tools = discover_tools_for_server(&service, server_name).await?;
            Ok((service, discovered_tools))
        }
        McpTransportSpec::Sse { .. } | McpTransportSpec::Http { .. } => {
            let config = build_http_transport_config(&spec)?;
            let transport = rmcp::transport::StreamableHttpClientTransport::from_config(config);
            let service = handler
                .connect(transport)
                .await
                .map_err(|e| format!("failed to connect http MCP server: {e}"))?;

            let discovered_tools = discover_tools_for_server(&service, server_name).await?;
            Ok((service, discovered_tools))
        }
    }
}

async fn discover_tools_for_server(
    service: &rmcp::service::RunningService<
        rmcp::service::RoleClient,
        rig::tool::rmcp::McpClientHandler,
    >,
    server_name: &str,
) -> Result<Vec<McpToolDefinition>, String> {
    let tools = service
        .peer()
        .list_all_tools()
        .await
        .map_err(|e| format!("failed to list MCP tools for server '{server_name}': {e}"))?;

    Ok(tools
        .into_iter()
        .map(|tool| McpToolDefinition {
            server: server_name.to_string(),
            name: tool.name.to_string(),
            description: tool.description.map(|d| d.to_string()),
            parameters: Some(serde_json::Value::Object((*tool.input_schema).clone())),
        })
        .collect())
}

#[cfg(test)]
mod test;
