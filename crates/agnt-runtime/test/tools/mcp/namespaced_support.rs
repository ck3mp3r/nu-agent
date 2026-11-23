//! Test support for the namespaced-tool suite: the `NamespacedTool` wrapper.
//!
//! The wrapper namespaces an inner `DynamicTool` with a server prefix. It has
//! no src consumer — the production path builds through `build_mcp_dynamic_tool`
//! — so it lives here, moved verbatim from `src/tools/mcp/namespaced.rs`.

use rig::tool::{DynamicTool, ToolExecutionError};

use crate::tools::limits::truncate_tool_output;

/// A wrapper around a DynamicTool that namespaces the tool name with a server prefix.
///
/// This ensures rig's ToolServer sees namespaced names (e.g., `server__tool`) instead of
/// raw MCP tool names, preventing name collisions across MCP servers.
pub(crate) struct NamespacedTool {
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
    pub(crate) fn new(
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
    pub(crate) fn name(&self) -> String {
        self.namespaced_name.clone()
    }

    /// The original tool description.
    pub(crate) fn description(&self) -> String {
        self.inner.definition().description
    }

    /// The tool parameters schema.
    pub(crate) fn parameters(&self) -> serde_json::Value {
        self.inner.definition().parameters
    }

    /// Execute the tool with the given JSON args string.
    pub(crate) async fn call(&self, args: String) -> Result<String, ToolExecutionError> {
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
