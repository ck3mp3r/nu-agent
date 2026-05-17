use rig::completion::ToolDefinition;
use rig::tool::{ToolDyn, ToolError};
use rig::wasm_compat::WasmBoxedFuture;
use rmcp::handler::client::ClientHandler;
use rmcp::model::ClientInfo;
use rmcp::service::{NotificationContext, RoleClient, RunningService, ServiceExt};
use rmcp::transport::IntoTransport;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A wrapper around any ToolDyn that namespaces the tool name with a server prefix.
///
/// This ensures rig's ToolServer sees namespaced names (e.g., `nu__run`) instead of
/// raw MCP tool names, preventing name collisions across MCP servers.
pub struct NamespacedTool {
    inner: Box<dyn ToolDyn>,
    namespaced_name: String,
}

impl NamespacedTool {
    /// Create a new NamespacedTool wrapping an inner tool.
    ///
    /// # Arguments
    /// * `inner` - The tool to wrap (any type implementing ToolDyn)
    /// * `server_prefix` - The server name prefix (e.g., "nu", "context7")
    /// * `delimiter` - The delimiter to use between prefix and tool name (e.g., "__")
    pub fn new(inner: Box<dyn ToolDyn>, server_prefix: &str, delimiter: &str) -> Self {
        let raw_name = inner.name();
        let namespaced_name = format!("{server_prefix}{delimiter}{raw_name}");
        Self {
            inner,
            namespaced_name,
        }
    }
}

impl ToolDyn for NamespacedTool {
    fn name(&self) -> String {
        self.namespaced_name.clone()
    }

    fn definition<'a>(&'a self, prompt: String) -> WasmBoxedFuture<'a, ToolDefinition> {
        // Get the definition from the inner tool, then override its name
        let inner_def_future = self.inner.definition(prompt);
        let namespaced_name = self.namespaced_name.clone();

        Box::pin(async move {
            let mut def = inner_def_future.await;
            def.name = namespaced_name;
            def
        })
    }

    fn call<'a>(&'a self, args: String) -> WasmBoxedFuture<'a, Result<String, ToolError>> {
        // Delegate the call to the inner tool unchanged
        self.inner.call(args)
    }
}

/// A ClientHandler implementation that wraps MCP tools with NamespacedTool before
/// registering them with rig's ToolServer.
///
/// This handler follows the same pattern as rig's McpClientHandler but adds a
/// namespacing layer to prevent tool name collisions across MCP servers.
pub struct NamespacedClientHandler {
    client_info: ClientInfo,
    tool_server_handle: rig::tool::server::ToolServerHandle,
    server_prefix: String,
    delimiter: String,
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
    pub fn new(
        client_info: ClientInfo,
        tool_server_handle: rig::tool::server::ToolServerHandle,
        server_prefix: String,
        delimiter: String,
    ) -> Self {
        Self {
            client_info,
            tool_server_handle,
            server_prefix,
            delimiter,
            managed_tool_names: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Connect to an MCP server via the given transport and register namespaced tools.
    ///
    /// This is analogous to McpClientHandler::connect but wraps tools in NamespacedTool.
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
                let mcp_tool =
                    rig::tool::rmcp::McpTool::from_mcp_server(tool, service.peer().clone());

                // Wrap the McpTool in a NamespacedTool
                let namespaced_tool = NamespacedTool::new(
                    Box::new(mcp_tool),
                    &handler.server_prefix,
                    &handler.delimiter,
                );

                // The namespaced name is what rig will see
                let namespaced_name = namespaced_tool.name();

                handler
                    .tool_server_handle
                    .add_tool(namespaced_tool)
                    .await
                    .map_err(McpClientError::ToolServerError)?;

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
            if let Err(e) = self.tool_server_handle.remove_tool(&name).await {
                log::error!("Failed to remove tool '{name}': {e}");
            }
        }

        // Register new tools (wrapped in NamespacedTool)
        for tool in tools {
            let mcp_tool = rig::tool::rmcp::McpTool::from_mcp_server(tool, context.peer.clone());

            let namespaced_tool =
                NamespacedTool::new(Box::new(mcp_tool), &self.server_prefix, &self.delimiter);

            let namespaced_name = namespaced_tool.name();

            if let Err(e) = self.tool_server_handle.add_tool(namespaced_tool).await {
                log::error!("Failed to add namespaced tool '{namespaced_name}': {e}");
                continue;
            }

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

    #[error("Tool server error: {0}")]
    ToolServerError(#[source] rig::tool::server::ToolServerError),
}

#[cfg(test)]
#[path = "namespaced_test.rs"]
mod namespaced_test;
