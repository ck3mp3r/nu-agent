use crate::tools::limits::truncate_tool_output;
use crate::types::ToolDefinition;
use rig::tool::{ToolDyn, ToolError};
use rig::wasm_compat::WasmBoxedFuture;
use std::path::PathBuf;

use crate::tools::handler::fs::dispatch_fs_tool;

/// Error type for builtin tool execution failures.
///
/// This error is used to wrap builtin tool execution errors so they can be
/// converted into rig's ToolError::ToolCallError.
#[derive(Debug, thiserror::Error)]
enum BuiltinExecError {
    #[error("Tool execution failed: {0}")]
    Execution(String),

    #[error("Argument parsing failed: {0}")]
    ArgumentParsing(String),

    #[error("Result serialization failed: {0}")]
    ResultSerialization(String),
}

/// Adapts a builtin tool to rig's ToolDyn interface.
///
/// This adapter bridges our builtin tools (read, edit, patch, skill, http) with rig's
/// dynamic tool system. It wraps a tool definition and the current working directory,
/// then dispatches calls to the appropriate builtin handler.
pub struct BuiltinToolAdapter {
    tool_def: ToolDefinition,
    cwd: PathBuf,
    max_tool_result_bytes: usize,
}

impl BuiltinToolAdapter {
    /// Create a new adapter for a builtin tool.
    ///
    /// # Arguments
    ///
    /// * `tool_def` - The tool definition (name, description, parameters schema)
    /// * `cwd` - The current working directory for resolving relative paths
    /// * `max_tool_result_bytes` - Maximum bytes before truncation (0 = disabled)
    pub fn new(tool_def: ToolDefinition, cwd: PathBuf, max_tool_result_bytes: usize) -> Self {
        Self {
            tool_def,
            cwd,
            max_tool_result_bytes,
        }
    }
}

impl ToolDyn for BuiltinToolAdapter {
    fn name(&self) -> String {
        self.tool_def.name.clone()
    }

    fn description(&self) -> String {
        self.tool_def.description.clone()
    }

    fn parameters(&self) -> serde_json::Value {
        self.tool_def.parameters.clone()
    }

    fn call<'a>(&'a self, args: String) -> WasmBoxedFuture<'a, Result<String, ToolError>> {
        Box::pin(async move {
            // Parse JSON arguments to serde_json::Value
            let args_json: serde_json::Value = serde_json::from_str(&args).map_err(|e| {
                ToolError::ToolCallError(Box::new(BuiltinExecError::ArgumentParsing(format!(
                    "Invalid JSON: {e}"
                ))))
            })?;

            // Dispatch based on tool name
            let result = if self.tool_def.name == "http" {
                crate::tools::handler::http::dispatch_http_tool(&args_json)
                    .await
                    .map_err(|e| {
                        ToolError::ToolCallError(Box::new(BuiltinExecError::Execution(format!(
                            "{}: {}",
                            e.message,
                            e.details
                                .map(|d| d.to_string())
                                .unwrap_or_else(|| "no details".to_string())
                        ))))
                    })?
            } else {
                // Call the builtin FS tool dispatcher
                dispatch_fs_tool(&self.tool_def.name, &args_json, &self.cwd).map_err(|e| {
                    ToolError::ToolCallError(Box::new(BuiltinExecError::Execution(format!(
                        "{}: {}",
                        e.message,
                        e.details
                            .map(|d| d.to_string())
                            .unwrap_or_else(|| "no details".to_string())
                    ))))
                })?
            };

            // The dispatcher returns Option<JsonValue>. If None, the tool wasn't recognized.
            let result_json = result.ok_or_else(|| {
                ToolError::ToolCallError(Box::new(BuiltinExecError::Execution(format!(
                    "Unknown builtin tool: {}",
                    self.tool_def.name
                ))))
            })?;

            // Serialize result to string and cap output size before returning to rig.
            let result_str = serde_json::to_string(&result_json).map_err(|e| {
                ToolError::ToolCallError(Box::new(BuiltinExecError::ResultSerialization(format!(
                    "JSON serialization failed: {e}"
                ))))
            })?;
            Ok(truncate_tool_output(result_str, self.max_tool_result_bytes))
        })
    }
}

/// Convert all builtin tool definitions to BuiltinToolAdapter instances.
///
/// This function creates a BuiltinToolAdapter for each builtin tool definition,
/// allowing them to be registered with rig's ToolServer.
///
/// # Arguments
///
/// * `tool_definitions` - Vector of tool definitions for builtin tools
/// * `cwd` - The current working directory for resolving relative paths
/// * `max_tool_result_bytes` - Maximum bytes before truncation (0 = disabled)
///
/// # Returns
///
/// A vector of BuiltinToolAdapter instances, one for each tool definition.
/// These can be passed directly to ToolServerHandle::add_tool() since they implement ToolDyn.
pub fn adapt_builtins(
    tool_definitions: Vec<ToolDefinition>,
    cwd: PathBuf,
    max_tool_result_bytes: usize,
) -> Vec<BuiltinToolAdapter> {
    tool_definitions
        .into_iter()
        .map(|tool_def| BuiltinToolAdapter::new(tool_def, cwd.clone(), max_tool_result_bytes))
        .collect()
}

#[cfg(test)]
#[path = "builtin_test.rs"]
mod builtin_adapter_test;
