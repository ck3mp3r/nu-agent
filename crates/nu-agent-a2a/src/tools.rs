use std::sync::Arc;

use rig::tool::{ToolDyn, ToolError};
use rig::wasm_compat::WasmBoxedFuture;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::*;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Error type for A2A tool adapter execution failures.
#[derive(Debug, thiserror::Error)]
enum A2aToolError {
    #[error("Argument parsing failed: {0}")]
    ArgumentParsing(String),
    #[error("Tool execution failed: {0}")]
    Execution(String),
    #[error("Result serialization failed: {0}")]
    ResultSerialization(String),
}

// ---------------------------------------------------------------------------
// A2aToolAdapter — bridges A2A tools into rig's ToolDyn system
// ---------------------------------------------------------------------------

/// Adapter that wraps an A2A tool for rig's ToolDyn registration system.
///
/// Each [`A2aToolDef`] is paired with an [`A2aToolContext`] so that rig's
/// [`ToolServerHandle`] can route LLM function-calls to the corresponding A2A
/// handler.
pub struct A2aToolAdapter {
    tool_def: A2aToolDef,
    ctx: A2aToolContext,
}

impl A2aToolAdapter {
    pub fn new(tool_def: A2aToolDef, ctx: A2aToolContext) -> Self {
        Self { tool_def, ctx }
    }
}

impl ToolDyn for A2aToolAdapter {
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
            let args_json: serde_json::Value = serde_json::from_str(&args).map_err(|e| {
                ToolError::ToolCallError(Box::new(A2aToolError::ArgumentParsing(format!(
                    "Invalid JSON arguments: {e}"
                ))))
            })?;

            let result = handle_dispatch(&self.tool_def.name, &self.ctx, args_json)
                .await
                .map_err(|e| ToolError::ToolCallError(Box::new(A2aToolError::Execution(e))))?;

            serde_json::to_string(&result).map_err(|e| {
                ToolError::ToolCallError(Box::new(A2aToolError::ResultSerialization(format!(
                    "Failed to serialize A2A tool result: {e}"
                ))))
            })
        })
    }
}

/// Register all A2A tools (agent.list, agent.getCard, tasks.send, etc.) on a
/// rig [`ToolServerHandle`].
///
/// This function blocks the current thread (via `runtime.block_on`) so it must
/// be called from a non-async context (e.g., a synchronous plugin command).
///
/// # Errors
///
/// Returns an error string if any tool cannot be registered on the server.
pub fn register_a2a_tools(
    tool_server_handle: &rig::tool::server::ToolServerHandle,
    ctx: A2aToolContext,
    runtime: &tokio::runtime::Runtime,
) -> Result<(), String> {
    for def in a2a_tool_defs() {
        let adapter = A2aToolAdapter::new(def, ctx.clone());
        runtime
            .block_on(async { tool_server_handle.add_tool(adapter).await })
            .map_err(|e| format!("Failed to register A2A tool: {e}"))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// A2aToolDef
// ---------------------------------------------------------------------------

/// A tool definition for LLM function-calling, describing an A2A operation the
/// model can invoke.
pub struct A2aToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Value, // JSON Schema
}

// ---------------------------------------------------------------------------
// A2aToolContext
// ---------------------------------------------------------------------------

/// Shared context passed to every A2A tool handler.
#[derive(Clone)]
pub struct A2aToolContext {
    pub client: A2aClient,
    pub cache: Arc<PeerCache>,
    pub own_card: AgentCard,
    pub task_store: Option<Arc<TaskStore>>,
    /// Channel to notify the runtime about completed background tasks.
    pub completion_tx: Option<mpsc::Sender<A2aCompletionEvent>>,
    /// Handle for spawning background SSE watcher tasks.
    pub runtime_handle: Option<tokio::runtime::Handle>,
}

type ToolResult = Result<Value, String>;

// ---------------------------------------------------------------------------
// Tool definition generation
// ---------------------------------------------------------------------------

/// Generate all A2A tool definitions for the LLM (currently 7).
pub fn a2a_tool_defs() -> Vec<A2aToolDef> {
    vec![
        A2aToolDef {
            name: "agent.list".into(),
            description: "List all discovered A2A agents on the local network. Returns agent names and URLs.".into(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        },
        A2aToolDef {
            name: "agent.getCard".into(),
            description: "Get the Agent Card for a specific peer by name. Use agent.list first to see available agents.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the agent to get the card for"
                    }
                },
                "required": ["name"]
            }),
        },
        A2aToolDef {
            name: "tasks.send".into(),
            description: "Send a task/message to another agent. The receiving agent will process it and call tasks.complete when done. Use agent.list to find available agents first.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Name of the target agent"
                    },
                    "text": {
                        "type": "string",
                        "description": "Message text to send"
                    },
                    "sessionId": {
                        "type": "string",
                        "description": "Optional session ID to group related tasks"
                    }
                },
                "required": ["target", "text"]
            }),
        },
        A2aToolDef {
            name: "tasks.get".into(),
            description: "Get the current status of a task sent to another agent. Check whether it was completed or is still in progress.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "string",
                        "description": "Task ID to query"
                    },
                    "target": {
                        "type": "string",
                        "description": "Name of the target agent"
                    }
                },
                "required": ["taskId", "target"]
            }),
        },
        A2aToolDef {
            name: "tasks.cancel".into(),
            description: "Cancel a task that was sent to another agent.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "string",
                        "description": "Task ID to cancel"
                    },
                    "target": {
                        "type": "string",
                        "description": "Name of the target agent"
                    }
                },
                "required": ["taskId", "target"]
            }),
        },
        A2aToolDef {
            name: "tasks.list".into(),
            description: "List tasks from all known peers, or filter by a specific target agent or task status.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Optional: filter tasks from a specific agent"
                    },
                    "status": {
                        "type": "string",
                        "description": "Optional: filter by task state (submitted, working, completed, failed, canceled, rejected)"
                    }
                }
            }),
        },
        A2aToolDef {
            name: "tasks.complete".into(),
            description:
                "Complete an A2A task by submitting the result. Call this when you have finished processing an incoming A2A task request. The taskId is provided in the [A2A Task ...] message that was injected into your conversation. Use the taskId from that message and provide your response as the result text."
                    .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "string",
                        "description": "Task ID to complete"
                    },
                    "result": {
                        "type": "string",
                        "description": "The result text produced by processing the task"
                    }
                },
                "required": ["taskId", "result"]
            }),
        },
    ]
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn handle_agent_list(ctx: &A2aToolContext) -> ToolResult {
    let peers = ctx.cache.list();
    let result: Vec<Value> = peers
        .iter()
        .map(|p| {
            serde_json::json!({
                "name": p.name,
                "url": p.url,
                "description": p.card.as_ref().and_then(|c| c.description.as_deref()),
                "skills": p.card.as_ref().map(|c| {
                    c.skills.iter().map(|s| &s.name).collect::<Vec<_>>()
                }),
            })
        })
        .collect();
    Ok(serde_json::json!({ "agents": result }))
}

pub async fn handle_agent_get_card(ctx: &A2aToolContext, params: Value) -> ToolResult {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'name'".to_string())?;

    match ctx.cache.get(name) {
        Some(peer) => {
            let card = match peer.card {
                Some(ref c) => serde_json::to_value(c).unwrap_or_else(
                    |e| serde_json::json!({"error": format!("card serialization failed: {e}")}),
                ),
                None => serde_json::json!({"name": peer.name}),
            };
            Ok(card)
        }
        None => Err(format!("Agent '{name}' not found")),
    }
}

pub async fn handle_tasks_send(ctx: &A2aToolContext, params: Value) -> ToolResult {
    let target = params
        .get("target")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'target'".to_string())?;
    let text = params
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'text'".to_string())?;
    let session_id = params
        .get("sessionId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let peer = ctx
        .cache
        .get(target)
        .ok_or_else(|| format!("Agent '{target}' not found"))?;

    let message = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: text.to_string(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let task = ctx
        .client
        .send_task(
            &peer.url,
            message,
            session_id,
            Some(ctx.own_card.url.clone()),
        )
        .await
        .map_err(|e| format!("A2A error: {e}"))?;

    // ── Background SSE watcher ──────────────────────────────────────────
    // If we have a completion channel and a runtime handle, spawn a
    // background task that subscribes to the remote task's SSE stream and
    // delivers a completion event back to the runtime when the task reaches
    // a terminal state.  This lets the LLM see the completion result on its
    // next turn without having to poll tasks.get.
    let agent_name = target.to_string();
    let client = ctx.client.clone();
    let url = peer.url.clone();
    let task_id = task.id.clone();

    if let Some(completion_tx) = ctx.completion_tx.clone()
        && let Some(runtime_handle) = ctx.runtime_handle.clone()
    {
        runtime_handle.spawn(async move {
            match client.subscribe_task(&url, &task_id).await {
                Ok(final_task) => {
                    let result = extract_text_from_task(&final_task);
                    let event = A2aCompletionEvent {
                        task_id: final_task.id,
                        agent_name,
                        result,
                        status: final_task.status.state,
                    };
                    let _ = completion_tx.send(event).await;
                }
                Err(e) => {
                    log::warn!("A2A task watcher failed for {task_id}: {e}");
                }
            }
        });
    }

    Ok(serde_json::json!({
        "taskId": task.id,
        "status": "sent",
        "message": format!("Task sent to {target}. You will be notified when it completes."),
    }))
}

/// Extract result text from a (presumably completed) task's artifacts.
///
/// Collects all [`Part::Text`] parts across all artifacts and joins them
/// with newlines.
fn extract_text_from_task(task: &Task) -> String {
    task.artifacts
        .iter()
        .flat_map(|a| a.parts.iter())
        .filter_map(|p| match p {
            Part::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn handle_tasks_get(ctx: &A2aToolContext, params: Value) -> ToolResult {
    let task_id = params
        .get("taskId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'taskId'".to_string())?;
    let target = params
        .get("target")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'target'".to_string())?;

    let peer = ctx
        .cache
        .get(target)
        .ok_or_else(|| format!("Agent '{target}' not found"))?;

    let task = ctx
        .client
        .get_task(&peer.url, task_id)
        .await
        .map_err(|e| format!("A2A error: {e}"))?;

    Ok(serde_json::json!({
        "taskId": task.id,
        "state": task.status.state,
        "artifacts": task.artifacts,
    }))
}

pub async fn handle_tasks_cancel(ctx: &A2aToolContext, params: Value) -> ToolResult {
    let task_id = params
        .get("taskId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'taskId'".to_string())?;
    let target = params
        .get("target")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'target'".to_string())?;

    let peer = ctx
        .cache
        .get(target)
        .ok_or_else(|| format!("Agent '{target}' not found"))?;

    let task = ctx
        .client
        .cancel_task(&peer.url, task_id)
        .await
        .map_err(|e| format!("A2A error: {e}"))?;

    Ok(serde_json::json!({
        "taskId": task.id,
        "state": task.status.state,
    }))
}

pub async fn handle_tasks_list(ctx: &A2aToolContext, params: Value) -> ToolResult {
    let status = params
        .get("status")
        .and_then(|v| v.as_str())
        .and_then(|s| match s {
            "submitted" => Some(TaskState::Submitted),
            "working" => Some(TaskState::Working),
            "inputRequired" => Some(TaskState::InputRequired),
            "completed" => Some(TaskState::Completed),
            "failed" => Some(TaskState::Failed),
            "canceled" => Some(TaskState::Canceled),
            "rejected" => Some(TaskState::Rejected),
            _ => None,
        });

    // If targeting a specific agent, delegate to the client
    if let Some(target) = params.get("target").and_then(|v| v.as_str()) {
        let peer = ctx
            .cache
            .get(target)
            .ok_or_else(|| format!("Agent '{target}' not found"))?;

        let tasks = ctx
            .client
            .list_tasks(&peer.url, status)
            .await
            .map_err(|e| format!("A2A error: {e}"))?;

        return Ok(serde_json::json!({ "tasks": tasks }));
    }

    // No target: list from local TaskStore via A2aToolContext
    match ctx.task_store.as_ref() {
        Some(store) => {
            let (tasks, _) = store.list_tasks_filtered(status, 50, None);
            Ok(serde_json::json!({ "tasks": tasks }))
        }
        None => Ok(serde_json::json!({ "tasks": [] })),
    }
}

pub async fn handle_tasks_complete(ctx: &A2aToolContext, params: Value) -> ToolResult {
    let task_id = params
        .get("taskId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'taskId'".to_string())?;
    let result = params
        .get("result")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'result'".to_string())?;

    let store = ctx.task_store.as_ref().ok_or_else(|| {
        "No local TaskStore available (this agent cannot complete tasks)".to_string()
    })?;

    store
        .complete_task(task_id, result)
        .map(|task| serde_json::json!({"taskId": task.id, "state": task.status.state}))
        .map_err(|e| format!("A2A error: {e}"))
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

async fn handle_dispatch(name: &str, ctx: &A2aToolContext, params: Value) -> ToolResult {
    match name {
        "agent.list" => handle_agent_list(ctx).await,
        "agent.getCard" => handle_agent_get_card(ctx, params).await,
        "tasks.send" => handle_tasks_send(ctx, params).await,
        "tasks.get" => handle_tasks_get(ctx, params).await,
        "tasks.cancel" => handle_tasks_cancel(ctx, params).await,
        "tasks.list" => handle_tasks_list(ctx, params).await,
        "tasks.complete" => handle_tasks_complete(ctx, params).await,
        _ => Err(format!("Unknown A2A tool: {name}")),
    }
}
