use crate::tools::limits::truncate_tool_output;
use rig::tool::{DynamicTool, ToolExecutionError, ToolOutput};
use rmcp::handler::client::ClientHandler;
use rmcp::model::ClientInfo;
use rmcp::service::{NotificationContext, RoleClient, RunningService, ServiceExt};
use rmcp::transport::IntoTransport;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Default per-call timeout for MCP tools (matches rig's DEFAULT_MCP_TOOL_TIMEOUT).
const DEFAULT_MCP_TOOL_TIMEOUT: Duration = Duration::from_secs(300);

/// A wrapper around a DynamicTool that namespaces the tool name with a server prefix.
///
/// This ensures rig's ToolServer sees namespaced names (e.g., `nu__run`) instead of
/// raw MCP tool names, preventing name collisions across MCP servers.
pub struct NamespacedTool {
    inner: DynamicTool,
    namespaced_name: String,
    max_tool_result_bytes: usize,
}

impl NamespacedTool {
    /// Create a new NamespacedTool wrapping an inner DynamicTool.
    ///
    /// # Arguments
    /// * `inner` - The tool to wrap
    /// * `server_prefix` - The server name prefix (e.g., "nu", "context7")
    /// * `delimiter` - The delimiter to use between prefix and tool name (e.g., "__")
    /// * `max_tool_result_bytes` - Maximum bytes before truncation (0 = disabled)
    pub fn new(
        inner: DynamicTool,
        server_prefix: &str,
        delimiter: &str,
        max_tool_result_bytes: usize,
    ) -> Self {
        let raw_name = inner.name().to_string();
        let namespaced_name = format!("{server_prefix}{delimiter}{raw_name}");
        Self {
            inner,
            namespaced_name,
            max_tool_result_bytes,
        }
    }

    /// The namespaced tool name.
    pub fn name(&self) -> String {
        self.namespaced_name.clone()
    }

    /// The original tool description.
    pub fn description(&self) -> String {
        self.inner.definition().description
    }

    /// The tool parameters schema.
    pub fn parameters(&self) -> serde_json::Value {
        self.inner.definition().parameters
    }

    /// Execute the tool with the given JSON args string.
    pub async fn call(&self, args: String) -> Result<String, ToolExecutionError> {
        // We need to execute through the inner tool. Since DynamicTool doesn't expose
        // a direct execute method, we use a ToolContext and execute via ToolSet.
        let mut context = rig::tool::ToolContext::new();
        let mut toolset = rig::tool::ToolSet::default();
        let raw_name = self.inner.name().to_string();
        toolset.add_dynamic_tool(self.inner.clone());
        let result = toolset.execute(&raw_name, &args, &mut context).await;

        if result.is_success() {
            Ok(truncate_tool_output(
                result.output().render(),
                self.max_tool_result_bytes,
            ))
        } else if let Some(error) = result.error() {
            Err(error.clone())
        } else {
            // Refusal or skipped — no output available
            Ok(String::new())
        }
    }
}
/// Build a DynamicTool that calls an MCP server tool via its ServerSink.
///
/// This replaces the old pattern of wrapping rig's (now private) `McpTool` in a
/// `NamespacedTool`. The tool name is already namespaced (e.g. `nu__run`).
fn build_mcp_dynamic_tool(
    name: String,
    description: String,
    parameters: serde_json::Value,
    client: rmcp::service::ServerSink,
    max_tool_result_bytes: usize,
    raw_name: String,
) -> DynamicTool {
    DynamicTool::new(name, description, parameters, move |_context, args| {
        let client = client.clone();
        let raw_name = raw_name.clone();
        Box::pin(async move {
            // Serialize args to string for MCP parsing
            let args_str = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());

            // Parse JSON arguments for MCP
            let arguments = parse_mcp_arguments(&args_str).map_err(|error| {
                ToolExecutionError::invalid_args(format!(
                    "MCP tool received invalid arguments: {error}"
                ))
                .with_source(error)
            })?;

            let request = arguments
                .map(|args| {
                    rmcp::model::CallToolRequestParams::new(raw_name.clone()).with_arguments(args)
                })
                .unwrap_or_else(|| rmcp::model::CallToolRequestParams::new(raw_name.clone()));

            // Call the MCP server
            let result = call_mcp_tool(&client, request, Some(DEFAULT_MCP_TOOL_TIMEOUT))
                .await
                .map_err(|error| {
                    ToolExecutionError::provider(format!("MCP tool request failed: {error}"))
                        .with_source(error)
                })?;

            let is_error = result.is_error == Some(true);
            let output = mcp_result_output(&result).map_err(|error| {
                ToolExecutionError::provider(format!("MCP tool result conversion failed: {error}"))
                    .with_source(error)
            })?;

            // Apply truncation
            let output = truncate_tool_output(output, max_tool_result_bytes);

            if is_error {
                Err(
                    ToolExecutionError::other("MCP tool reported an execution error".to_string())
                        .with_model_output(ToolOutput::text(output)),
                )
            } else {
                Ok(ToolOutput::text(output))
            }
        })
    })
}

/// Parse JSON args string into MCP-compatible arguments.
fn parse_mcp_arguments(args: &str) -> Result<Option<rmcp::model::JsonObject>, serde_json::Error> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let value: serde_json::Value = serde_json::from_str(trimmed)?;
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Object(_) => Ok(Some(serde_json::from_value(value)?)),
        _ => Ok(None),
    }
}

/// Call an MCP tool and return the result.
async fn call_mcp_tool(
    peer: &rmcp::service::ServerSink,
    params: rmcp::model::CallToolRequestParams,
    timeout: Option<Duration>,
) -> Result<rmcp::model::CallToolResult, rmcp::ServiceError> {
    use rmcp::model::{CallToolRequest, ClientRequest, ServerResult};

    let request = ClientRequest::CallToolRequest(CallToolRequest::new(params));
    let handle = match timeout {
        Some(duration) => tokio::time::timeout(
            duration,
            peer.send_cancellable_request(request, rmcp::service::PeerRequestOptions::no_options()),
        )
        .await
        .map_err(|_| rmcp::ServiceError::Timeout { timeout: duration })??,
        None => {
            peer.send_cancellable_request(request, rmcp::service::PeerRequestOptions::no_options())
                .await?
        }
    };

    let response = handle.await_response().await?;
    match response {
        ServerResult::CallToolResult(result) => Ok(result),
        _ => Err(rmcp::ServiceError::UnexpectedResponse),
    }
}

/// Convert MCP CallToolResult content blocks to a string output.
fn mcp_result_output(result: &rmcp::model::CallToolResult) -> Result<String, ToolExecutionError> {
    let mut parts: Vec<String> = Vec::new();
    for block in &result.content {
        match block {
            rmcp::model::ContentBlock::Text(text) => parts.push(text.text.clone()),
            rmcp::model::ContentBlock::Image(image) => {
                parts.push(format!("[image: {}]", image.mime_type));
            }
            rmcp::model::ContentBlock::Audio(audio) => {
                parts.push(format!("[audio: {}]", audio.mime_type));
            }
            rmcp::model::ContentBlock::Resource(resource) => {
                let uri = match &resource.resource {
                    rmcp::model::ResourceContents::TextResourceContents { uri, .. } => uri.clone(),
                    rmcp::model::ResourceContents::BlobResourceContents { uri, .. } => uri.clone(),
                    _ => "[unknown resource]".to_string(),
                };
                parts.push(format!("[resource: {uri}]"));
            }
            rmcp::model::ContentBlock::ResourceLink(link) => {
                parts.push(format!("[resource_link: {}]", link.uri));
            }
            _ => {
                // For unknown content types, serialize as JSON
                if let Ok(json) = serde_json::to_string(block) {
                    parts.push(json);
                }
            }
        }
    }
    Ok(parts.join("\n"))
}

/// A ClientHandler implementation that wraps MCP tools with DynamicTool before
/// registering them with rig's ToolServer.
///
/// This handler follows the same pattern as rig's McpClientHandler but adds a
/// namespacing layer to prevent tool name collisions across MCP servers.
pub struct NamespacedClientHandler {
    client_info: ClientInfo,
    tool_server_handle: rig::tool::server::ToolServerHandle,
    server_prefix: String,
    delimiter: String,
    max_tool_result_bytes: usize,
    /// Stores the NAMESPACED tool names that we manage
    managed_tool_names: Arc<RwLock<Vec<String>>>,
}

impl NamespacedClientHandler {
    /// Create a new NamespacedClientHandler.
    ///
    /// # Arguments
    /// * `client_info` - MCP client information
    /// * `tool_server_handle` - Handle to rig's ToolServer for registering tools
    /// * `server_prefix` - The server name to use as prefix (e.g., "nu", "context7")
    /// * `delimiter` - The delimiter between prefix and tool name (typically "__")
    /// * `max_tool_result_bytes` - Maximum bytes before truncation (0 = disabled)
    pub fn new(
        client_info: ClientInfo,
        tool_server_handle: rig::tool::server::ToolServerHandle,
        server_prefix: String,
        delimiter: String,
        max_tool_result_bytes: usize,
    ) -> Self {
        Self {
            client_info,
            tool_server_handle,
            server_prefix,
            delimiter,
            max_tool_result_bytes,
            managed_tool_names: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Connect to an MCP server via the given transport and register namespaced tools.
    ///
    /// This is analogous to McpClientHandler::connect but wraps tools in DynamicTool.
    ///
    /// Returns both the running service and the raw tool list from the MCP server.
    /// The raw tools are needed by callers to build metadata without a second round-trip.
    pub async fn connect<T, E, A>(
        self,
        transport: T,
    ) -> Result<(RunningService<RoleClient, Self>, Vec<rmcp::model::Tool>), McpClientError>
    where
        T: IntoTransport<RoleClient, E, A>,
        E: Error + Send + Sync + 'static,
    {
        let service = ServiceExt::serve(self, transport)
            .await
            .map_err(|e| McpClientError::ServiceError(Box::new(e)))?;

        let tools = service
            .peer()
            .list_all_tools()
            .await
            .map_err(McpClientError::ListToolsError)?;

        // Clone tools before consuming them in the loop
        let raw_tools = tools.clone();

        {
            let handler = service.service();
            let mut managed = handler.managed_tool_names.write().await;

            for tool in tools {
                let namespaced_name = format!(
                    "{}{}{}",
                    handler.server_prefix, handler.delimiter, tool.name
                );

                let dynamic_tool = build_mcp_dynamic_tool(
                    namespaced_name.clone(),
                    tool.description.clone().unwrap_or_default().to_string(),
                    tool.schema_as_json_value(),
                    service.peer().clone(),
                    handler.max_tool_result_bytes,
                    tool.name.to_string(),
                );

                handler
                    .tool_server_handle
                    .add_dynamic_tool(dynamic_tool)
                    .await;

                // Store the namespaced name for later removal
                managed.push(namespaced_name);
            }
        }

        Ok((service, raw_tools))
    }
}

impl ClientHandler for NamespacedClientHandler {
    fn get_info(&self) -> ClientInfo {
        self.client_info.clone()
    }

    async fn on_tool_list_changed(&self, context: NotificationContext<RoleClient>) {
        // Fetch updated tool list
        let tools = match context.peer.list_all_tools().await {
            Ok(tools) => tools,
            Err(e) => {
                log::error!("Failed to list tools on tool_list_changed: {e}");
                return;
            }
        };

        // Remove all previously managed tools (by NAMESPACED names)
        let mut managed = self.managed_tool_names.write().await;
        for name in managed.drain(..) {
            self.tool_server_handle.remove_tool(&name).await;
        }

        // Register new tools (wrapped in DynamicTool)
        for tool in tools {
            let namespaced_name = format!("{}{}{}", self.server_prefix, self.delimiter, tool.name);

            let dynamic_tool = build_mcp_dynamic_tool(
                namespaced_name.clone(),
                tool.description.clone().unwrap_or_default().to_string(),
                tool.schema_as_json_value(),
                context.peer.clone(),
                self.max_tool_result_bytes,
                tool.name.to_string(),
            );

            self.tool_server_handle.add_dynamic_tool(dynamic_tool).await;

            managed.push(namespaced_name);
        }
    }
}

/// Error types for NamespacedClientHandler operations
#[derive(Debug, thiserror::Error)]
pub enum McpClientError {
    #[error("Service error: {0}")]
    ServiceError(#[source] Box<dyn Error + Send + Sync>),

    #[error("Failed to list tools: {0}")]
    ListToolsError(#[source] rmcp::service::ServiceError),
}

#[cfg(test)]
#[path = "namespaced_test.rs"]
mod namespaced_test;
