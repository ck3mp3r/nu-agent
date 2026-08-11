use crate::tools::limits::truncate_tool_output;
use crate::types::ToolDefinition;
use rig::tool::{DynamicTool, ToolExecutionError, ToolOutput};
use std::path::PathBuf;

use crate::tools::handler::fs::dispatch_fs_tool;
use crate::tools::handler::nu::dispatch_nu_tool;
use crate::tools::handler::tmux::dispatch_tmux_tool;

/// Adapts a builtin tool to rig's DynamicTool interface.
///
/// This adapter bridges our builtin tools (read, edit, patch, skill, http) with rig's
/// dynamic tool system. It wraps a tool definition and the current working directory,
/// then dispatches calls to the appropriate builtin handler.
pub struct BuiltinToolAdapter {
    pub(crate) tool_def: ToolDefinition,
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

    /// Convert this adapter into a DynamicTool for registration with rig.
    pub fn into_dynamic_tool(self) -> DynamicTool {
        let name = self.tool_def.name.clone();
        let description = self.tool_def.description.clone();
        let parameters = self.tool_def.parameters.clone();
        let cwd = self.cwd;
        let max_tool_result_bytes = self.max_tool_result_bytes;
        let is_http = name == "http";
        let is_tmux = name.starts_with("tmux_");
        let is_nu = name == "nu";

        DynamicTool::new(
            name.clone(),
            description,
            parameters,
            move |_context, args| {
                let cwd = cwd.clone();
                let name = name.clone();
                Box::pin(async move {
                    // Dispatch based on tool name
                    let result = if is_http {
                        crate::tools::handler::http::dispatch_http_tool(&args)
                            .await
                            .map_err(|e| {
                                ToolExecutionError::provider(format!(
                                    "{}: {}",
                                    e.message,
                                    e.details
                                        .map(|d| d.to_string())
                                        .unwrap_or_else(|| "no details".to_string())
                                ))
                            })?
                    } else if is_tmux {
                        // Call the builtin tmux tool dispatcher
                        dispatch_tmux_tool(&name, &args, &cwd).map_err(|e| {
                            ToolExecutionError::provider(format!(
                                "{}: {}",
                                e.message,
                                e.details
                                    .map(|d| d.to_string())
                                    .unwrap_or_else(|| "no details".to_string())
                            ))
                        })?
                    } else if is_nu {
                        // Call the builtin nu tool dispatcher
                        dispatch_nu_tool(&name, &args, &cwd).map_err(|e| {
                            ToolExecutionError::provider(format!(
                                "{}: {}",
                                e.message,
                                e.details
                                    .map(|d| d.to_string())
                                    .unwrap_or_else(|| "no details".to_string())
                            ))
                        })?
                    } else {
                        // Call the builtin FS tool dispatcher
                        dispatch_fs_tool(&name, &args, &cwd).map_err(|e| {
                            ToolExecutionError::provider(format!(
                                "{}: {}",
                                e.message,
                                e.details
                                    .map(|d| d.to_string())
                                    .unwrap_or_else(|| "no details".to_string())
                            ))
                        })?
                    };

                    // The dispatcher returns Option<JsonValue>. If None, the tool wasn't recognized.
                    let result_json = result.ok_or_else(|| {
                        ToolExecutionError::not_found(format!("Unknown builtin tool: {}", name))
                    })?;

                    // Serialize result to string and cap output size before returning to rig.
                    let result_str = serde_json::to_string(&result_json).map_err(|e| {
                        ToolExecutionError::other(format!("JSON serialization failed: {e}"))
                    })?;
                    Ok(ToolOutput::text(truncate_tool_output(
                        result_str,
                        max_tool_result_bytes,
                    )))
                })
            },
        )
    }
}

/// Convert all builtin tool definitions to DynamicTool instances.
///
/// This function creates a DynamicTool for each builtin tool definition,
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
/// A vector of DynamicTool instances, one for each tool definition.
/// These can be passed directly to ToolServerHandle::add_dynamic_tool().
pub fn adapt_builtins(
    tool_definitions: Vec<ToolDefinition>,
    cwd: PathBuf,
    max_tool_result_bytes: usize,
) -> Vec<DynamicTool> {
    tool_definitions
        .into_iter()
        .map(|tool_def| {
            BuiltinToolAdapter::new(tool_def, cwd.clone(), max_tool_result_bytes)
                .into_dynamic_tool()
        })
        .collect()
}

#[cfg(test)]
#[path = "builtin_test.rs"]
mod builtin_adapter_test;
