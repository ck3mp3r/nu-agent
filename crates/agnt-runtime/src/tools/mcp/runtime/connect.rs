//! The connection arms: one call per server (stdio, or HTTP/SSE with bearer or
//! OAuth auth), the batch that connects the enabled set, and the tool
//! definitions it discovers.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use crate::config::defaults;
use crate::config::vault::Vault;
use crate::tools::mcp::MCP_TOOL_NAMESPACE_DELIMITER;
use crate::tools::mcp::client::McpToolDefinition;
use crate::tools::mcp::config::{McpAuthConfig, McpServerConfig, McpTransportType};
use crate::tools::mcp::namespaced::{McpClientError, NamespacedClientHandler};

use super::McpRuntime;
use super::McpSessionHandle;
use super::collapse_failure_reason;
use super::http::{build_http_transport_config, build_mcp_http_client};
use super::servers::{
    compose_exposed_tool_name, register_exposed_name, select_enabled_servers,
    validate_raw_tool_name,
};
use super::stdio::{merged_stdio_env_with_pwd, resolve_caller_cwd, resolve_stdio_cwd};

// region:    --- Constants

/// Upper bound on one server's startup handshake.
///
/// rmcp applies no timeout of its own: the stdio child transport carries none,
/// and a peer-request timeout is applied only when a caller configures one. A
/// child that never answers the `initialize` request would therefore park
/// startup forever, so every arm waits at most this long. The HTTP arms keep
/// their client-level connect and read timeouts; this bound is defense in
/// depth there.
pub const CONNECT_DEADLINE: Duration = Duration::from_secs(10);

// endregion: --- Constants

pub async fn connect_servers(
    tool_server_handle: &rig::tool::server::ToolServerHandle,
    servers: &[McpServerConfig],
    caller_cwd: Option<&std::path::Path>,
    max_tool_result_bytes: usize,
    vault: Option<&Arc<Vault>>,
    connect_deadline: Duration,
) -> Result<McpRuntime, String> {
    let mut sessions = Vec::new();
    let mut connected_servers = std::collections::BTreeSet::new();
    let mut discovered_tools = Vec::new();
    let mut failures = std::collections::BTreeMap::new();
    let mut exposed_name_owner: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for server in select_enabled_servers(servers) {
        let (service, server_tools) = match connect_server(
            tool_server_handle,
            server,
            caller_cwd,
            max_tool_result_bytes,
            vault,
            connect_deadline,
        )
        .await
        {
            Ok(connected) => connected,
            Err(message) => {
                log::warn!(
                    "MCP server '{}' unavailable at startup: {message}",
                    server.name
                );
                // The surface gets the reason, not only the log: one row it
                // can clip, while the log keeps the whole message.
                failures.insert(server.name.clone(), collapse_failure_reason(&message));
                continue;
            }
        };

        for tool in &server_tools {
            register_exposed_name(&mut exposed_name_owner, &tool.name, &server.name)?;
        }

        connected_servers.insert(server.name.clone());
        discovered_tools.extend(server_tools);
        sessions.push(McpSessionHandle::Rmcp { _service: service });
    }

    Ok(McpRuntime {
        sessions,
        connected_servers,
        discovered_tools,
        failures,
    })
}

pub(crate) async fn connect_server(
    tool_server_handle: &rig::tool::server::ToolServerHandle,
    server: &McpServerConfig,
    caller_cwd: Option<&std::path::Path>,
    max_tool_result_bytes: usize,
    vault: Option<&Arc<Vault>>,
    connect_deadline: Duration,
) -> Result<
    (
        rmcp::service::RunningService<rmcp::service::RoleClient, NamespacedClientHandler>,
        Vec<McpToolDefinition>,
    ),
    String,
> {
    let server_name = server.name.as_str();
    let client_config = rmcp::model::ClientConfig::new(
        rmcp::model::ClientCapabilities::default(),
        rmcp::model::Implementation::new("nu-agent", env!("CARGO_PKG_VERSION")),
    );
    let handler = NamespacedClientHandler::new(
        client_config,
        tool_server_handle.clone(),
        server.name.clone(),
        MCP_TOOL_NAMESPACE_DELIMITER.to_string(),
        max_tool_result_bytes,
    );

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

            // The server runs at the shared child-priority niceness. `nice` execs
            // it in place, so rmcp's child identity, pipes, and lifecycle are
            // unchanged; a spawn retry is impossible here, which is why the policy
            // probes PATH instead.
            let mut cmd = crate::child_priority::ChildPriority::probe().command(&command);
            for arg in args {
                cmd.arg(arg);
            }
            cmd.current_dir(&cwd);

            env = merged_stdio_env_with_pwd(env, &cwd, &caller);

            for (k, v) in env {
                cmd.env(k, v);
            }
            let transport = spawn_stdio_transport(cmd, server_name)?;

            let (service, raw_tools) =
                bounded_connect("stdio", connect_deadline, handler.connect(transport)).await?;

            let discovered_tools = build_tool_definitions(server_name, raw_tools)?;
            Ok((service, discovered_tools))
        }
        McpTransportType::Sse | McpTransportType::Http => {
            let config = build_http_transport_config(server, vault.map(Arc::as_ref))?;

            match &server.auth {
                McpAuthConfig::None | McpAuthConfig::Bearer { .. } => {
                    // EXISTING PATH — no change
                    let transport = rmcp::transport::StreamableHttpClientTransport::with_client(
                        build_mcp_http_client(defaults::MCP_READ_TIMEOUT_SECS)?,
                        config,
                    );
                    let (service, raw_tools) =
                        bounded_connect("http", connect_deadline, handler.connect(transport))
                            .await?;

                    let discovered_tools = build_tool_definitions(server_name, raw_tools)?;
                    Ok((service, discovered_tools))
                }
                McpAuthConfig::OAuth {
                    client_id,
                    client_secret,
                    scope: _,
                    redirect_uri,
                } => {
                    let server_url = server.url.clone().ok_or_else(|| {
                        format!("MCP server '{}' with OAuth auth requires url", server.name)
                    })?;

                    // Validate the server URL for SSRF protection
                    crate::tools::mcp::safe_http_client::validate_url(&server_url).map_err(
                        |e| format!("Invalid MCP server URL for '{}': {e}", server.name),
                    )?;

                    // The vault supplies per-server credential and state adapters.
                    let vault = vault.ok_or_else(|| {
                        format!(
                            "MCP server '{}' requires OAuth but no vault is configured",
                            server.name
                        )
                    })?;
                    let vault_cred_store = vault.mcp_credential_store(&server.name);
                    let vault_state_store = vault.mcp_state_store();

                    // Create AuthorizationManager
                    let mut auth_manager = rmcp::transport::AuthorizationManager::new(&server_url)
                        .await
                        .map_err(|e| {
                            format!("failed to create auth manager for '{}': {e}", server.name)
                        })?;

                    // Set credential and state stores
                    auth_manager.set_credential_store(vault_cred_store);
                    auth_manager.set_state_store(vault_state_store);

                    // Resolve OAuth metadata (required before the manager can use stored credentials)
                    let resolution = auth_manager.resolve_metadata().await.map_err(|e| {
                        format!(
                            "failed to discover OAuth metadata for '{}': {e}",
                            server.name
                        )
                    })?;
                    auth_manager.set_metadata(resolution.metadata);

                    // Configure the OAuth client with client_id and client_secret from config.
                    // The redirect_uri is not used at runtime (no callback server), but
                    // OAuthClientConfig requires it. Use the config value or a placeholder.
                    if let Some(cid) = client_id {
                        let rd_uri = redirect_uri
                            .as_deref()
                            .unwrap_or("http://127.0.0.1:0/mcp/oauth/callback");
                        let mut client_config =
                            rmcp::transport::auth::OAuthClientConfig::new(cid.clone(), rd_uri);
                        if let Some(secret) = client_secret {
                            client_config = client_config.with_client_secret(secret.clone());
                        }
                        auth_manager.configure_client(client_config).map_err(|e| {
                            format!(
                                "failed to configure OAuth client for '{}': {e}",
                                server.name
                            )
                        })?;
                    }
                    // If no client_id in config, the client was registered during login.
                    // The stored client_info in the credential store has the client_id.
                    // rmcp's AuthorizationManager loads it from the vault credential adapter.

                    // Build HTTP client and wrap with AuthClient
                    let http_client = build_mcp_http_client(defaults::MCP_READ_TIMEOUT_SECS)?;
                    let auth_client = rmcp::transport::AuthClient::new(http_client, auth_manager);

                    let transport = rmcp::transport::StreamableHttpClientTransport::with_client(
                        auth_client,
                        config,
                    );
                    let (service, raw_tools) =
                        bounded_connect("http", connect_deadline, handler.connect(transport))
                            .await?;

                    let discovered_tools = build_tool_definitions(server_name, raw_tools)?;
                    Ok((service, discovered_tools))
                }
            }
        }
    }
}

// region:    --- Support

/// Spawn the stdio transport for `cmd` with the child's stderr piped and pumped
/// into the log.
///
/// The child is a foreign program: it writes its own diagnostics — a clap usage
/// dump, a panic trace, a warning — to stderr. Inheriting stderr would let those
/// bytes reach the terminal the TUI owns and paint outside the frame, so the
/// transport pipes them and this seam forwards each line to the log sink, tagged
/// with its server.
fn spawn_stdio_transport(
    cmd: tokio::process::Command,
    server_name: &str,
) -> Result<rmcp::transport::TokioChildProcess, String> {
    let (transport, child_stderr) = rmcp::transport::TokioChildProcess::builder(cmd)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to build stdio transport: {e}"))?;
    if let Some(child_stderr) = child_stderr {
        spawn_stderr_pump(server_name.to_string(), child_stderr);
    }
    Ok(transport)
}

/// Pump the child's stderr into the log, one warn per line, tagged with its
/// server. The task ends at EOF, when the child closes the stream.
fn spawn_stderr_pump(server_name: String, child_stderr: tokio::process::ChildStderr) {
    tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(child_stderr);
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            match tokio::io::AsyncBufReadExt::read_until(&mut reader, b'\n', &mut buffer).await {
                Ok(0) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buffer);
                    let line = line.trim_end_matches(['\r', '\n']);
                    if !line.is_empty() {
                        log::warn!("MCP server '{server_name}': {line}");
                    }
                }
                Err(e) => {
                    log::debug!("MCP server '{server_name}': stderr read failed: {e}");
                    break;
                }
            }
        }
    });
}

/// Await one server's startup under `deadline`.
///
/// `connect` covers rmcp's whole startup: the `initialize` request, the
/// initialized notification, and the first `tools/list`. A timeout reports the
/// bound; the caller adds the server name to the startup warning.
async fn bounded_connect<T>(
    transport: &str,
    deadline: Duration,
    connect: impl Future<Output = Result<T, McpClientError>>,
) -> Result<T, String> {
    match tokio::time::timeout(deadline, connect).await {
        Ok(Ok(connected)) => Ok(connected),
        Ok(Err(e)) => Err(format!("failed to connect {transport} MCP server: {e}")),
        Err(_) => Err(format!(
            "initialize handshake did not complete within {deadline:?}"
        )),
    }
}

fn build_tool_definitions(
    server_name: &str,
    raw_tools: Vec<rmcp::model::Tool>,
) -> Result<Vec<McpToolDefinition>, String> {
    let mut discovered_tools = Vec::with_capacity(raw_tools.len());
    for tool in raw_tools {
        let raw_name = tool.name.to_string();
        validate_raw_tool_name(server_name, &raw_name)?;

        discovered_tools.push(McpToolDefinition {
            raw_name: raw_name.clone(),
            server: server_name.to_string(),
            name: compose_exposed_tool_name(server_name, &raw_name),
            description: tool.description.map(|d| d.to_string()),
            parameters: Some(serde_json::Value::Object((*tool.input_schema).clone())),
        });
    }
    Ok(discovered_tools)
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../../../test/tools/mcp/runtime/connect_stderr.rs"]
mod connect_stderr_test;

// endregion: --- Tests
