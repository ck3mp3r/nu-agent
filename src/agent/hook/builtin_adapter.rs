use rig::completion::ToolDefinition;
use rig::tool::{ToolDyn, ToolError};
use rig::wasm_compat::WasmBoxedFuture;
use std::path::PathBuf;

use crate::agent::tools::handler::builtin_fs::dispatch_builtin_fs_tool;

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

/// Adapts a builtin FS tool to rig's ToolDyn interface.
///
/// This adapter bridges our builtin FS tools (read, edit, patch, skill) with rig's
/// dynamic tool system. It wraps a tool definition and the current working directory,
/// then dispatches calls to the appropriate builtin handler.
pub struct BuiltinToolAdapter {
    tool_def: ToolDefinition,
    cwd: PathBuf,
}

impl BuiltinToolAdapter {
    /// Create a new adapter for a builtin FS tool.
    ///
    /// # Arguments
    ///
    /// * `tool_def` - The tool definition (name, description, parameters schema)
    /// * `cwd` - The current working directory for resolving relative paths
    pub fn new(tool_def: ToolDefinition, cwd: PathBuf) -> Self {
        Self { tool_def, cwd }
    }
}

impl ToolDyn for BuiltinToolAdapter {
    fn name(&self) -> String {
        self.tool_def.name.clone()
    }

    fn definition<'a>(&'a self, _prompt: String) -> WasmBoxedFuture<'a, ToolDefinition> {
        let def = self.tool_def.clone();
        Box::pin(async move { def })
    }

    fn call<'a>(&'a self, args: String) -> WasmBoxedFuture<'a, Result<String, ToolError>> {
        Box::pin(async move {
            // Parse JSON arguments to serde_json::Value
            let args_json: serde_json::Value = serde_json::from_str(&args).map_err(|e| {
                ToolError::ToolCallError(Box::new(BuiltinExecError::ArgumentParsing(format!(
                    "Invalid JSON: {e}"
                ))))
            })?;

            // Call the builtin FS tool dispatcher
            let result = dispatch_builtin_fs_tool(&self.tool_def.name, &args_json, &self.cwd)
                .map_err(|e| {
                    ToolError::ToolCallError(Box::new(BuiltinExecError::Execution(format!(
                        "{}: {}",
                        e.message,
                        e.details
                            .map(|d| d.to_string())
                            .unwrap_or_else(|| "no details".to_string())
                    ))))
                })?;

            // The dispatcher returns Option<JsonValue>. If None, the tool wasn't recognized.
            let result_json = result.ok_or_else(|| {
                ToolError::ToolCallError(Box::new(BuiltinExecError::Execution(format!(
                    "Unknown builtin tool: {}",
                    self.tool_def.name
                ))))
            })?;

            // Serialize result to string
            serde_json::to_string(&result_json).map_err(|e| {
                ToolError::ToolCallError(Box::new(BuiltinExecError::ResultSerialization(format!(
                    "JSON serialization failed: {e}"
                ))))
            })
        })
    }
}

/// Convert all builtin FS tool definitions to BuiltinToolAdapter instances.
///
/// This function creates a BuiltinToolAdapter for each builtin tool definition,
/// allowing them to be registered with rig's ToolServer.
///
/// # Arguments
///
/// * `tool_definitions` - Vector of tool definitions for builtin FS tools
/// * `cwd` - The current working directory for resolving relative paths
///
/// # Returns
///
/// A vector of BuiltinToolAdapter instances, one for each tool definition.
/// These can be passed directly to ToolServerHandle::add_tool() since they implement ToolDyn.
pub fn adapt_builtins(
    tool_definitions: Vec<ToolDefinition>,
    cwd: PathBuf,
) -> Vec<BuiltinToolAdapter> {
    tool_definitions
        .into_iter()
        .map(|tool_def| BuiltinToolAdapter::new(tool_def, cwd.clone()))
        .collect()
}

#[cfg(test)]
#[path = "builtin_adapter_test.rs"]
mod builtin_adapter_test;
