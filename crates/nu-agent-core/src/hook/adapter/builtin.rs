use crate::tools::limits::truncate_tool_output;
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
    socket_dir: PathBuf,
    agent_name: Option<String>,
    max_tool_result_bytes: usize,
}

impl BuiltinToolAdapter {
    /// Create a new adapter for a builtin tool.
    ///
    /// # Arguments
    ///
    /// * `tool_def` - The tool definition (name, description, parameters schema)
    /// * `cwd` - The current working directory for resolving relative paths
    /// * `orchestrator` - Optional orchestrator state for spawn_agent and list_agents
    /// * `socket_dir` - Socket directory for send_message delivery
    /// * `agent_name` - Optional agent identity for send_message `from` field
    /// * `max_tool_result_bytes` - Maximum bytes before truncation (0 = disabled)
    pub fn new(
        tool_def: ToolDefinition,
        cwd: PathBuf,
        orchestrator: Option<
            Arc<std::sync::Mutex<crate::tools::handler::spawn_agent::OrchestratorState>>,
        >,
        socket_dir: PathBuf,
        agent_name: Option<String>,
        max_tool_result_bytes: usize,
    ) -> Self {
        Self {
            tool_def,
            cwd,
            orchestrator,
            socket_dir,
            agent_name,
            max_tool_result_bytes,
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

                // Lock, run the sync phase, then drop the guard before awaiting.
                // MutexGuard is !Send and cannot be held across .await on a multi-threaded runtime.
                let prepared = {
                    let mut state = orchestrator.lock().unwrap();
                    crate::tools::handler::spawn_agent::prepare_spawn_agent(
                        &args_json,
                        &mut state,
                        &crate::tools::handler::spawn_agent::RealTmuxRunner,
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
                };
                // Guard dropped — safe to .await
                crate::tools::handler::spawn_agent::finish_spawn_agent(
                    prepared,
                    &crate::tools::handler::spawn_agent::RealTmuxRunner,
                )
                .await
                .map(Some)
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
                let from = self.agent_name.as_deref().unwrap_or("unknown");
                let to = args_json["to"].as_str().ok_or_else(|| {
                    ToolError::ToolCallError(Box::new(BuiltinExecError::Execution(
                        "send_message: missing 'to'".to_string(),
                    )))
                })?;
                let message = args_json["message"].as_str().ok_or_else(|| {
                    ToolError::ToolCallError(Box::new(BuiltinExecError::Execution(
                        "send_message: missing 'message'".to_string(),
                    )))
                })?;
                let kind = args_json["kind"].as_str().unwrap_or("message");
                crate::mailbox::send_to(&self.socket_dir, to, from, message, kind)
                    .await
                    .map_err(|e| {
                        ToolError::ToolCallError(Box::new(BuiltinExecError::Execution(format!(
                            "{e}"
                        ))))
                    })?;
                Some(serde_json::json!({ "sent": true }))
            } else if self.tool_def.name == "list_agents" {
                // Lists all .sock files in socket_dir — intentionally includes
                // the caller's own socket so the LLM has full visibility of all
                // agents in this workspace, including itself.
                use crate::tools::handler::spawn_agent::is_pane_alive;

                let socket_names: Vec<String> = std::fs::read_dir(&self.socket_dir)
                    .map(|entries| {
                        entries
                            .flatten()
                            .filter_map(|e| {
                                let name = e.file_name().to_string_lossy().to_string();
                                name.strip_suffix(".sock").map(String::from)
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                // Snapshot agent_panes from OrchestratorState if available
                let agent_panes_snapshot: std::collections::HashMap<String, String> = self
                    .orchestrator
                    .as_ref()
                    .map(|o| {
                        let state = o.lock().unwrap();
                        state.agent_panes.clone()
                    })
                    .unwrap_or_default();

                let tmux = crate::tools::handler::spawn_agent::RealTmuxRunner;
                let agents: Vec<serde_json::Value> = socket_names
                    .into_iter()
                    .map(|name| {
                        let pane_id = agent_panes_snapshot.get(&name).cloned();
                        let pane_alive = pane_id
                            .as_deref()
                            .map(|pid| is_pane_alive(&tmux, pid))
                            .unwrap_or(false);
                        serde_json::json!({
                            "name": name,
                            "pane_id": pane_id,
                            "pane_alive": pane_alive,
                        })
                    })
                    .collect();

                // Auto-cleanup: remove dead pane entries from OrchestratorState.
                // Agents with pane_id known but pane gone are cleaned up so they
                // don't accumulate indefinitely. Agents without pane_id (not spawned
                // by this orchestrator) are never removed.
                let dead_names: Vec<String> = agents
                    .iter()
                    .filter(|a| {
                        a["pane_id"] != serde_json::Value::Null
                            && !a["pane_alive"].as_bool().unwrap_or(true)
                    })
                    .filter_map(|a| a["name"].as_str().map(String::from))
                    .collect();

                if !dead_names.is_empty()
                    && let Some(ref orchestrator) = self.orchestrator
                {
                    let mut state = orchestrator.lock().unwrap();
                    for name in &dead_names {
                        state.agent_panes.remove(name);
                        log::info!("Auto-cleaned dead agent '{}' (pane no longer alive)", name);
                    }
                }

                Some(serde_json::json!(agents))
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
/// * `orchestrator` - Optional orchestrator state for spawn_agent and list_agents
/// * `socket_dir` - Socket directory for send_message and list_agents
/// * `agent_name` - Optional agent identity for send_message `from` field
/// * `max_tool_result_bytes` - Maximum bytes before truncation (0 = disabled)
///
/// # Returns
///
/// A vector of BuiltinToolAdapter instances, one for each tool definition.
/// These can be passed directly to ToolServerHandle::add_tool() since they implement ToolDyn.
pub fn adapt_builtins(
    tool_definitions: Vec<ToolDefinition>,
    cwd: PathBuf,
    orchestrator: Option<
        Arc<std::sync::Mutex<crate::tools::handler::spawn_agent::OrchestratorState>>,
    >,
    socket_dir: PathBuf,
    agent_name: Option<String>,
    max_tool_result_bytes: usize,
) -> Vec<BuiltinToolAdapter> {
    tool_definitions
        .into_iter()
        .map(|tool_def| {
            BuiltinToolAdapter::new(
                tool_def,
                cwd.clone(),
                orchestrator.clone(),
                socket_dir.clone(),
                agent_name.clone(),
                max_tool_result_bytes,
            )
        })
        .collect()
}

#[cfg(test)]
#[path = "builtin_test.rs"]
mod builtin_adapter_test;
