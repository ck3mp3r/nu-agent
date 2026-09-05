use std::sync::Arc;

use http::{HeaderName, HeaderValue};
use tokio::sync::Mutex;

use crate::config::defaults;
use crate::tools::mcp::{
    MCP_TOOL_NAMESPACE_DELIMITER,
    client::McpToolDefinition,
    config::{McpAuthConfig, McpServerConfig, McpTransportType},
    credentials::{FileCredentialStore, FileStateStore, McpCredentialsStore},
    namespaced::NamespacedClientHandler,
};

pub struct McpRuntime {
    sessions: Vec<McpSessionHandle>,
    connected_servers: std::collections::BTreeSet<String>,
    discovered_tools: Vec<McpToolDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerLifecycle {
    pub name: String,
    pub configured: bool,
    pub enabled: bool,
    pub connected: bool,
    pub visible_tool_count: usize,
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
    Rmcp {
        _service: rmcp::service::RunningService<rmcp::service::RoleClient, NamespacedClientHandler>,
    },
}

fn select_enabled_servers(servers: &[McpServerConfig]) -> Vec<&McpServerConfig> {
    servers.iter().filter(|server| server.enabled).collect()
}

fn project_server_lifecycle(
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
        })
        .collect();

    projection.sort_by(|a, b| a.name.cmp(&b.name));
    projection
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
        // When auth field is set, skip Authorization header — auth takes precedence
        if name.eq_ignore_ascii_case("Authorization") && !matches!(server.auth, McpAuthConfig::None)
        {
            log::warn!(
                "MCP server '{}' has both 'auth' and 'headers.Authorization' configured. \
                 The 'auth' field takes precedence.",
                server.name
            );
            continue;
        }
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| format!("invalid MCP header name '{name}': {e}"))?;
        let header_value = HeaderValue::from_str(&value)
            .map_err(|e| format!("invalid MCP header value for '{name}': {e}"))?;
        custom_headers.insert(header_name, header_value);
    }

    let mut config =
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(url)
            .custom_headers(custom_headers);
    config.allow_stateless = allow_stateless;

    match &server.auth {
        McpAuthConfig::Bearer { token } => {
            config = config.auth_header(token.clone());
        }
        McpAuthConfig::None | McpAuthConfig::OAuth { .. } => {
            // No config-level auth header for None or OAuth
        }
    }

    Ok(config)
}

fn build_mcp_http_client(read_timeout_secs: u64) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .pool_max_idle_per_host(0) // match rmcp's default_http_client() — avoids Delayed ACK stall
        .pool_idle_timeout(Some(std::time::Duration::from_secs(55))); // evict before server closes
    if read_timeout_secs > 0 {
        builder = builder.read_timeout(std::time::Duration::from_secs(read_timeout_secs));
    }
    builder
        .build()
        .map_err(|e| format!("failed to build MCP HTTP client: {e}"))
}

impl McpRuntime {
    pub fn has_sessions(&self) -> bool {
        !self.sessions.is_empty()
    }

    pub fn discovered_tools(&self) -> &[McpToolDefinition] {
        &self.discovered_tools
    }

    pub fn lifecycle_projection(
        &self,
        configured_servers: &[McpServerConfig],
    ) -> Vec<McpServerLifecycle> {
        project_server_lifecycle(configured_servers, &self.connected_servers)
    }

    /// Returns true if a session for `server_name` was established (i.e. the server
    /// was connected at some point and its session is still alive).
    pub fn has_server(&self, server_name: &str) -> bool {
        self.connected_servers.contains(server_name)
    }

    /// Merge a newly-connected runtime into this one.
    ///
    /// Adds the new runtime's sessions, connected-server names, and discovered tools
    /// to this runtime. Used when enabling a previously-unconfigured server.
    pub fn merge(&mut self, other: McpRuntime) {
        self.sessions.extend(other.sessions);
        self.connected_servers.extend(other.connected_servers);
        self.discovered_tools.extend(other.discovered_tools);
    }

    /// Mark a server as disconnected — removes it from `connected_servers`.
    /// Called when a transport error is detected and the server is being disabled.
    /// The `McpSessionHandle` stays in `sessions` (sessions are one-shot; full
    /// reconnect requires an agent restart). Tool calls will fail until reconnected.
    pub fn mark_disconnected(&mut self, server_name: &str) {
        self.connected_servers.remove(server_name);
    }
}

pub async fn connect_servers(
    tool_server_handle: &rig::tool::server::ToolServerHandle,
    servers: &[McpServerConfig],
    caller_cwd: Option<&std::path::Path>,
    max_tool_result_bytes: usize,
) -> Result<McpRuntime, String> {
    // Check if any server uses OAuth — if so, load the shared credential store
    let needs_oauth = servers
        .iter()
        .any(|s| matches!(s.auth, McpAuthConfig::OAuth { .. }));
    let shared_cred_store: Option<Arc<Mutex<McpCredentialsStore>>> = if needs_oauth {
        Some(Arc::new(Mutex::new(
            McpCredentialsStore::load().unwrap_or_default(),
        )))
    } else {
        None
    };

    let mut sessions = Vec::new();
    let mut connected_servers = std::collections::BTreeSet::new();
    let mut discovered_tools = Vec::new();
    let mut exposed_name_owner: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for server in select_enabled_servers(servers) {
        let (service, server_tools) = connect_server(
            tool_server_handle,
            server,
            caller_cwd,
            max_tool_result_bytes,
            shared_cred_store.as_ref(),
        )
        .await?;

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
    })
}

pub(crate) async fn connect_server(
    tool_server_handle: &rig::tool::server::ToolServerHandle,
    server: &McpServerConfig,
    caller_cwd: Option<&std::path::Path>,
    max_tool_result_bytes: usize,
    shared_cred_store: Option<&Arc<Mutex<McpCredentialsStore>>>,
) -> Result<
    (
        rmcp::service::RunningService<rmcp::service::RoleClient, NamespacedClientHandler>,
        Vec<McpToolDefinition>,
    ),
    String,
> {
    let server_name = server.name.as_str();
    let client_info = rmcp::model::ClientInfo::new(
        rmcp::model::ClientCapabilities::default(),
        rmcp::model::Implementation::new("nu-agent", env!("CARGO_PKG_VERSION")),
    );
    let handler = NamespacedClientHandler::new(
        client_info,
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

            let (service, raw_tools) = handler
                .connect(transport)
                .await
                .map_err(|e| format!("failed to connect stdio MCP server: {e}"))?;

            let discovered_tools = build_tool_definitions(server_name, raw_tools)?;
            Ok((service, discovered_tools))
        }
        McpTransportType::Sse | McpTransportType::Http => {
            let config = build_http_transport_config(server)?;

            match &server.auth {
                McpAuthConfig::None | McpAuthConfig::Bearer { .. } => {
                    // EXISTING PATH — no change
                    let transport = rmcp::transport::StreamableHttpClientTransport::with_client(
                        build_mcp_http_client(defaults::MCP_READ_TIMEOUT_SECS)?,
                        config,
                    );
                    let (service, raw_tools) = handler
                        .connect(transport)
                        .await
                        .map_err(|e| format!("failed to connect http MCP server: {e}"))?;

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

                    // Load the shared credential store
                    let shared_store = shared_cred_store.as_ref().ok_or_else(|| {
                        format!(
                            "MCP server '{}' requires OAuth but credential store not initialized",
                            server.name
                        )
                    })?;

                    // Create per-server FileCredentialStore and FileStateStore
                    let file_cred_store =
                        FileCredentialStore::new((*shared_store).clone(), &server.name);
                    let file_state_store = FileStateStore::new((*shared_store).clone());

                    // Create AuthorizationManager
                    let mut auth_manager = rmcp::transport::AuthorizationManager::new(&server_url)
                        .await
                        .map_err(|e| {
                            format!("failed to create auth manager for '{}': {e}", server.name)
                        })?;

                    // Set credential and state stores
                    auth_manager.set_credential_store(file_cred_store);
                    auth_manager.set_state_store(file_state_store);

                    // Discover OAuth metadata (required before the manager can use stored credentials)
                    let metadata = auth_manager.discover_metadata().await.map_err(|e| {
                        format!(
                            "failed to discover OAuth metadata for '{}': {e}",
                            server.name
                        )
                    })?;
                    auth_manager.set_metadata(metadata);

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
                    // rmcp's AuthorizationManager loads it from FileCredentialStore.

                    // Build HTTP client and wrap with AuthClient
                    let http_client = build_mcp_http_client(defaults::MCP_READ_TIMEOUT_SECS)?;
                    let auth_client = rmcp::transport::AuthClient::new(http_client, auth_manager);

                    let transport = rmcp::transport::StreamableHttpClientTransport::with_client(
                        auth_client,
                        config,
                    );
                    let (service, raw_tools) = handler
                        .connect(transport)
                        .await
                        .map_err(|e| format!("failed to connect http MCP server: {e}"))?;

                    let discovered_tools = build_tool_definitions(server_name, raw_tools)?;
                    Ok((service, discovered_tools))
                }
            }
        }
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

#[cfg(test)]
#[path = "runtime_test.rs"]
mod runtime_test;
