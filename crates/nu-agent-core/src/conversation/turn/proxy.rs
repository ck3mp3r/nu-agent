//! `FilteredToolProxy` — a proxy tool that forwards `call` to an existing
//! `ToolServerHandle` while providing a pre-filtered `ToolDefinition`.

use crate::bus::Bus;
use crate::types::ToolDefinition;
use rig::tool::{DynamicTool, ToolExecutionError, ToolOutput, ToolResult};

/// A proxy tool that forwards `call` to an existing `ToolServerHandle`
/// while providing a pre-filtered `ToolDefinition`.
///
/// This allows the agent builder to use `.tools()` (which controls what
/// the LLM sees) while dispatching execution through the original shared
/// tool server (which has all registered tool implementations).
pub(crate) struct FilteredToolProxy {
    pub(crate) tool_name: String,
    pub(crate) tool_definition: ToolDefinition,
    pub(crate) handle: rig::tool::server::ToolServerHandle,
    pub(crate) bus: Bus,
}

impl FilteredToolProxy {
    /// Convert this proxy into a DynamicTool for registration with rig.
    pub(crate) fn into_dynamic_tool(self) -> DynamicTool {
        let name = self.tool_name.clone();
        let description = self.tool_definition.description.clone();
        let parameters = self.tool_definition.parameters.clone();
        let handle = self.handle;
        let bus = self.bus;

        let tool_name_for_closure = name.clone();
        DynamicTool::new(name, description, parameters, move |context, args| {
            let handle = handle.clone();
            let bus = bus.clone();
            let tool_name = tool_name_for_closure.clone();
            Box::pin(async move {
                // Serialize args back to string for the handle.execute call
                let args_str = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
                let mut cancel_rx = bus.cancel().subscribe();
                tokio::select! {
                    result = handle.execute(&tool_name, &args_str, context) => {
                        map_tool_result(&result)
                    }
                    Ok(_) = cancel_rx.recv() => {
                        Err(ToolExecutionError::cancelled("tool call cancelled"))
                    }
                }
            })
        })
    }
}

// region:    --- Support

/// Map a canonical `ToolResult` to the dynamic-tool call outcome.
///
/// Successful results pass their output through unchanged, failed results
/// surface the structured execution error, and refusals or skips become the
/// exact model-facing marker `"[refused]"` instead of an empty text block, so
/// intentional refusals stay distinguishable from missing output.
fn map_tool_result(result: &ToolResult) -> Result<ToolOutput, ToolExecutionError> {
    if result.is_success() {
        Ok(result.output().clone())
    } else if let Some(error) = result.error() {
        Err(error.clone())
    } else {
        Ok(ToolOutput::text("[refused]"))
    }
}

// endregion: --- Support

#[cfg(test)]
#[path = "proxy_test.rs"]
mod proxy_test;
