use http::{HeaderName, HeaderValue};
use rig::tool::server::ToolServer;

use crate::tools::mcp::{
    client::McpToolDefinition,
    config::{McpServerConfig, McpTransportType},
    MCP_TOOL_NAMESPACE_DELIMITER,
};

pub struct McpRuntime {
    tool_server_handle: rig::tool::server::ToolServerHandle,
    sessions: Vec<McpSessionHandle>,
    discovered_tools: Vec<McpToolDefinition>,
}

fn resolve_stdio_cwd(
    caller_cwd: &std::path::Path,
    cwd_override: Option<String>,
    server_name: &str,
) -> Result<std::path::PathBuf, String> {
    let canonical_caller = std::fs::canonicalize(caller_cwd).map_err(|e| {
        format!(
            "invalid caller cwd for stdio MCP server '{}': {} ({e})",
            server_name,
            caller_cwd.display()
        )
    })?;

    if !canonical_caller.is_dir() {
        return Err(format!(
            "invalid caller cwd for stdio MCP server '{}': '{}' is not a directory",
            server_name,
            canonical_caller.display()
        ));
    }

    if let Some(override_cwd) = cwd_override {
        let trimmed = override_cwd.trim();
        if trimmed.is_empty() {
            return Err(format!(
                "invalid stdio cwd override for MCP server '{}': path is empty",
                server_name
            ));
        }

        let path = std::path::PathBuf::from(trimmed);
        let effective_path = if path.is_absolute() {
            path.clone()
        } else {
            canonical_caller.join(&path)
        };

        let canonical = std::fs::canonicalize(&effective_path).map_err(|e| {
            format!(
                "invalid stdio cwd override for MCP server '{}': {} ({e})",
                server_name,
                effective_path.display()
            )
        })?;

        if !canonical.is_dir() {
            return Err(format!(
                "invalid stdio cwd override for MCP server '{}': '{}' is not a directory",
                server_name,
                canonical.display()
            ));
        }

        return Ok(canonical);
    }

    Ok(canonical_caller)
}

fn resolve_caller_cwd(
    caller_cwd: Option<&std::path::Path>,
    server_name: &str,
) -> Result<std::path::PathBuf, String> {
    let caller = caller_cwd.ok_or_else(|| {
        format!(
            "missing caller cwd for stdio MCP server '{}': provide invocation cwd",
            server_name
        )
    })?;

    resolve_stdio_cwd(caller, None, server_name)
}

fn merged_stdio_env_with_pwd(
    mut env: std::collections::HashMap<String, String>,
    effective_cwd: &std::path::Path,
    caller_cwd: &std::path::Path,
) -> std::collections::HashMap<String, String> {
    env.insert(
        "PWD".to_string(),
        effective_cwd.to_string_lossy().to_string(),
    );
    env.insert(
        "NU_AGENT_CALLER_CWD".to_string(),
        caller_cwd.to_string_lossy().to_string(),
    );
    env.insert(
        "NU_AGENT_CALLER_PWD".to_string(),
        caller_cwd.to_string_lossy().to_string(),
    );
    env
}

enum McpSessionHandle {
    #[allow(dead_code)]
    Rmcp(
        rmcp::service::RunningService<rmcp::service::RoleClient, rig::tool::rmcp::McpClientHandler>,
    ),
}

fn compose_exposed_tool_name(server_key: &str, raw_tool_name: &str) -> String {
    format!("{server_key}{MCP_TOOL_NAMESPACE_DELIMITER}{raw_tool_name}")
}

fn validate_raw_tool_name(server_name: &str, raw_tool_name: &str) -> Result<(), String> {
    if raw_tool_name.contains(MCP_TOOL_NAMESPACE_DELIMITER) {
        return Err(format!(
            "MCP server '{}' advertised tool '{}' containing reserved delimiter '{}'",
            server_name, raw_tool_name, MCP_TOOL_NAMESPACE_DELIMITER
        ));
    }

    Ok(())
}

fn register_exposed_name(
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

fn build_http_transport_config(
    server: &McpServerConfig,
) -> Result<rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig, String> {
    let (url, headers, allow_stateless) = match server.transport {
        McpTransportType::Sse => (
            server.url.clone().ok_or_else(|| {
                format!(
                    "MCP server '{}' with transport 'sse' requires url",
                    server.name
                )
            })?,
            server.headers.clone(),
            true,
        ),
        McpTransportType::Http => (
            server.url.clone().ok_or_else(|| {
                format!(
                    "MCP server '{}' with transport 'http' requires url",
                    server.name
                )
            })?,
            server.headers.clone(),
            false,
        ),
        McpTransportType::Stdio => {
            return Err("invalid transport type for HTTP config".to_string());
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

pub async fn connect_servers(
    servers: &[McpServerConfig],
    caller_cwd: Option<&std::path::Path>,
) -> Result<McpRuntime, String> {
    let tool_server_handle = ToolServer::new().run();

    let mut sessions = Vec::new();
    let mut discovered_tools = Vec::new();
    let mut exposed_name_owner: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for server in servers {
        let (service, server_tools) =
            connect_server(&tool_server_handle, server, caller_cwd).await?;

        for tool in &server_tools {
            register_exposed_name(&mut exposed_name_owner, &tool.name, &server.name)?;
        }

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
    server: &McpServerConfig,
    caller_cwd: Option<&std::path::Path>,
) -> Result<
    (
        rmcp::service::RunningService<rmcp::service::RoleClient, rig::tool::rmcp::McpClientHandler>,
        Vec<McpToolDefinition>,
    ),
    String,
> {
    let server_name = server.name.as_str();
    let client_info = rmcp::model::ClientInfo::new(
        rmcp::model::ClientCapabilities::default(),
        rmcp::model::Implementation::new("nu-agent", env!("CARGO_PKG_VERSION")),
    );
    let handler = rig::tool::rmcp::McpClientHandler::new(client_info, tool_server_handle.clone());

    match server.transport {
        McpTransportType::Stdio => {
            let command = server.command.clone().ok_or_else(|| {
                format!(
                    "MCP server '{}' with transport 'stdio' requires command",
                    server_name
                )
            })?;
            let args = server.args.clone();
            let mut env = server.env.clone();
            let caller = resolve_caller_cwd(caller_cwd, server_name)?;
            let cwd = resolve_stdio_cwd(caller.as_path(), server.cwd.clone(), server_name)?;

            let mut cmd = tokio::process::Command::new(command);
            for arg in args {
                cmd.arg(arg);
            }
            cmd.current_dir(&cwd);

            env = merged_stdio_env_with_pwd(env, &cwd, &caller);

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
        McpTransportType::Sse | McpTransportType::Http => {
            let config = build_http_transport_config(server)?;
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

    let mut discovered = Vec::with_capacity(tools.len());
    for tool in tools {
        let raw_name = tool.name.to_string();
        validate_raw_tool_name(server_name, &raw_name)?;

        discovered.push(McpToolDefinition {
            raw_name: raw_name.clone(),
            server: server_name.to_string(),
            name: compose_exposed_tool_name(server_name, &raw_name),
            description: tool.description.map(|d| d.to_string()),
            parameters: Some(serde_json::Value::Object((*tool.input_schema).clone())),
        });
    }

    Ok(discovered)
}

#[cfg(test)]
mod test;
