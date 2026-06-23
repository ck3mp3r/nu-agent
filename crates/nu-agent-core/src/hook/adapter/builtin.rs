use crate::types::ToolDefinition;
use rig::tool::{ToolDyn, ToolError};
use rig::wasm_compat::WasmBoxedFuture;
use std::path::PathBuf;
use std::sync::Arc;

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
/// This adapter bridges our builtin tools (read, edit, patch, skill, spawn_agent, terminate_agent, send_message, list_agents) with rig's
/// dynamic tool system. It wraps a tool definition and the current working directory,
/// then dispatches calls to the appropriate builtin handler.
pub struct BuiltinToolAdapter {
    tool_def: ToolDefinition,
    cwd: PathBuf,
    orchestrator:
        Option<Arc<std::sync::Mutex<crate::tools::handler::spawn_agent::OrchestratorState>>>,
    broker_sender: Option<Arc<tokio::sync::Mutex<crate::mailbox::BrokerSender>>>,
    agent_name: Option<String>,
}

impl BuiltinToolAdapter {
    /// Create a new adapter for a builtin tool.
    ///
    /// # Arguments
    ///
    /// * `tool_def` - The tool definition (name, description, parameters schema)
    /// * `cwd` - The current working directory for resolving relative paths
    /// * `orchestrator` - Optional orchestrator state for spawn_agent, send_message (parent), and list_agents
    /// * `broker_sender` - Optional broker sender for send_message (children)
    /// * `agent_name` - Optional agent identity for send_message `from` field
    #[allow(private_interfaces)]
    pub fn new(
        tool_def: ToolDefinition,
        cwd: PathBuf,
        orchestrator: Option<
            Arc<std::sync::Mutex<crate::tools::handler::spawn_agent::OrchestratorState>>,
        >,
        broker_sender: Option<Arc<tokio::sync::Mutex<crate::mailbox::BrokerSender>>>,
        agent_name: Option<String>,
    ) -> Self {
        Self {
            tool_def,
            cwd,
            orchestrator,
            broker_sender,
            agent_name,
        }
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

            // Dispatch based on tool name
            let result = if self.tool_def.name == "spawn_agent" {
                // spawn_agent requires orchestrator state
                let orchestrator = self.orchestrator.as_ref().ok_or_else(|| {
                    ToolError::ToolCallError(Box::new(BuiltinExecError::Execution(
                        "spawn_agent is only available to orchestrator agents".to_string(),
                    )))
                })?;

                let mut state = orchestrator.lock().unwrap();
                crate::tools::handler::spawn_agent::dispatch_spawn_agent(&args_json, &mut state)
                    .map_err(|e| {
                        ToolError::ToolCallError(Box::new(BuiltinExecError::Execution(format!(
                            "{}: {}",
                            e.message,
                            e.details
                                .map(|d| d.to_string())
                                .unwrap_or_else(|| "no details".to_string())
                        ))))
                    })?
            } else if self.tool_def.name == "terminate_agent" {
                // terminate_agent requires orchestrator state
                let orchestrator = self.orchestrator.as_ref().ok_or_else(|| {
                    ToolError::ToolCallError(Box::new(BuiltinExecError::Execution(
                        "terminate_agent is only available to orchestrator agents".to_string(),
                    )))
                })?;

                let mut state = orchestrator.lock().unwrap();
                crate::tools::handler::spawn_agent::dispatch_terminate_agent(&args_json, &mut state)
                    .map_err(|e| {
                        ToolError::ToolCallError(Box::new(BuiltinExecError::Execution(format!(
                            "{}: {}",
                            e.message,
                            e.details
                                .map(|d| d.to_string())
                                .unwrap_or_else(|| "no details".to_string())
                        ))))
                    })?
            } else if self.tool_def.name == "send_message" {
                // send_message: prefer broker_sender (child), fallback to orchestrator registry (parent)
                if let Some(sender) = &self.broker_sender {
                    let mut sender_guard = sender.lock().await;
                    crate::tools::handler::messaging::dispatch_send_message(
                        &args_json,
                        &mut sender_guard,
                    )
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
                } else if let Some(orchestrator) = &self.orchestrator {
                    let state = orchestrator.lock().unwrap();
                    let from = self.agent_name.as_deref().unwrap_or("orchestrator");
                    crate::tools::handler::messaging::dispatch_send_message_via_registry(
                        &args_json,
                        &state.registry,
                        from,
                    )
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
                    return Err(ToolError::ToolCallError(Box::new(
                        BuiltinExecError::Execution(
                            "send_message requires either broker_sender or orchestrator state"
                                .to_string(),
                        ),
                    )));
                }
            } else if self.tool_def.name == "list_agents" {
                // list_agents requires orchestrator state (parent only)
                let orchestrator = self.orchestrator.as_ref().ok_or_else(|| {
                    ToolError::ToolCallError(Box::new(BuiltinExecError::Execution(
                        "list_agents requires orchestrator state".to_string(),
                    )))
                })?;

                let state = orchestrator.lock().unwrap();
                crate::tools::handler::messaging::dispatch_list_agents(&state.registry).map_err(
                    |e| {
                        ToolError::ToolCallError(Box::new(BuiltinExecError::Execution(format!(
                            "{}: {}",
                            e.message,
                            e.details
                                .map(|d| d.to_string())
                                .unwrap_or_else(|| "no details".to_string())
                        ))))
                    },
                )?
            } else if self.tool_def.name == "http" {
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

            // Serialize result to string
            serde_json::to_string(&result_json).map_err(|e| {
                ToolError::ToolCallError(Box::new(BuiltinExecError::ResultSerialization(format!(
                    "JSON serialization failed: {e}"
                ))))
            })
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
/// * `orchestrator` - Optional orchestrator state for spawn_agent, send_message (parent), and list_agents
/// * `broker_sender` - Optional broker sender for send_message (children)
/// * `agent_name` - Optional agent identity for send_message `from` field
///
/// # Returns
///
/// A vector of BuiltinToolAdapter instances, one for each tool definition.
/// These can be passed directly to ToolServerHandle::add_tool() since they implement ToolDyn.
#[allow(private_interfaces)]
pub fn adapt_builtins(
    tool_definitions: Vec<ToolDefinition>,
    cwd: PathBuf,
    orchestrator: Option<
        Arc<std::sync::Mutex<crate::tools::handler::spawn_agent::OrchestratorState>>,
    >,
    broker_sender: Option<Arc<tokio::sync::Mutex<crate::mailbox::BrokerSender>>>,
    agent_name: Option<String>,
) -> Vec<BuiltinToolAdapter> {
    tool_definitions
        .into_iter()
        .map(|tool_def| {
            BuiltinToolAdapter::new(
                tool_def,
                cwd.clone(),
                orchestrator.clone(),
                broker_sender.clone(),
                agent_name.clone(),
            )
        })
        .collect()
}

#[cfg(test)]
#[path = "builtin_test.rs"]
mod builtin_adapter_test;
