use std::sync::Arc;

use rig::tool::{ToolDyn, ToolError};
use rig::wasm_compat::WasmBoxedFuture;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::*;

mod agent;
mod cancel;
mod get;
mod get_card;
mod list;
mod register;
mod send;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

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
// ToolResult
// ---------------------------------------------------------------------------

pub type ToolResult = Result<Value, String>;

// ---------------------------------------------------------------------------
// Tool — enum dispatch, no Box<dyn>
// ---------------------------------------------------------------------------

/// A registered A2A tool with metadata.
#[derive(Clone, Copy)]
pub enum Tool {
    Send,
    Get,
    List,
    Cancel,
    AgentList,
    GetCard,
}

impl Tool {
    pub fn name(&self) -> &'static str {
        match self {
            Tool::Send => "tasks_send",
            Tool::Get => "tasks_get",
            Tool::List => "tasks_list",
            Tool::Cancel => "tasks_cancel",
            Tool::AgentList => "agent_list",
            Tool::GetCard => "agent_getCard",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Tool::Send => {
                "Send a task to another agent over A2A. Do NOT poll tasks_get or tasks_list for completion — wait for the SSE notification (the tool will inform you when done)."
            }
            Tool::Get => {
                "Get a completed task from the LOCAL task store by ID (use AFTER the SSE notification arrives, NEVER for polling). Only returns tasks sent TO this agent."
            }
            Tool::List => {
                "List LOCAL tasks (use AFTER the SSE notification arrives, NEVER for polling). Only shows tasks sent TO this agent."
            }
            Tool::Cancel => "Cancel a running task",
            Tool::AgentList => "List all connected A2A agents and their URLs",
            Tool::GetCard => "Get the A2A agent card for a specific peer (or the local agent)",
        }
    }

    pub fn parameters(&self) -> Value {
        // Same as current register.rs parameters() function
        match self {
            Tool::Send => serde_json::json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string", "description": "Name of the target agent"},
                    "text": {"type": "string", "description": "Task text to send"}
                },
                "required": ["target", "text"]
            }),
            Tool::Get => serde_json::json!({
                "type": "object",
                "properties": {
                    "taskId": {"type": "string", "description": "Task ID to query"},
                    "target": {"type": "string", "description": "Name of the target agent"}
                },
                "required": ["taskId", "target"]
            }),
            Tool::List => serde_json::json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string", "description": "Optional: filter tasks from a specific agent"},
                    "status": {"type": "string", "description": "Optional: filter by task state (submitted, working, completed, failed, canceled, rejected)"}
                }
            }),
            Tool::Cancel => serde_json::json!({
                "type": "object",
                "properties": {
                    "taskId": {"type": "string", "description": "Task ID to cancel"},
                    "target": {"type": "string", "description": "Name of the target agent"}
                },
                "required": ["taskId", "target"]
            }),
            Tool::AgentList => serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            Tool::GetCard => serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Name of the agent to get the card for"}
                },
                "required": ["name"]
            }),
        }
    }

    pub async fn handle(&self, ctx: A2aToolContext, params: Value) -> ToolResult {
        match self {
            Tool::Send => send::handle(ctx, params).await,
            Tool::Get => get::handle(ctx, params).await,
            Tool::List => list::handle(ctx, params).await,
            Tool::Cancel => cancel::handle(ctx, params).await,
            Tool::AgentList => agent::handle(ctx, params).await,
            Tool::GetCard => get_card::handle(ctx, params).await,
        }
    }
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

    fn parameters(&self) -> Value {
        self.tool_def.parameters.clone()
    }

    fn call<'a>(&'a self, args: String) -> WasmBoxedFuture<'a, Result<String, ToolError>> {
        Box::pin(async move {
            let args_json: Value = serde_json::from_str(&args).map_err(|e| {
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

// ---------------------------------------------------------------------------
// A2aToolDef
// ---------------------------------------------------------------------------

/// A tool definition for LLM function-calling, describing an A2A operation the
/// model can invoke.
pub struct A2aToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
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
    pub task_store: Option<Arc<InMemoryTaskStore>>,
    /// Channel to notify the runtime about completed background tasks.
    pub completion_tx: Option<mpsc::Sender<A2aCompletionEvent>>,
    /// Handle for spawning background SSE watcher tasks.
    pub runtime_handle: Option<tokio::runtime::Handle>,
}

// ---------------------------------------------------------------------------
// Tool table (lazy)
// ---------------------------------------------------------------------------

fn tool_table() -> &'static Vec<Tool> {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Vec<Tool>> = OnceLock::new();
    TABLE.get_or_init(register::register_a2a_tools)
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

async fn handle_dispatch(name: &str, ctx: &A2aToolContext, params: Value) -> ToolResult {
    let tools = tool_table();
    let tool = tools
        .iter()
        .find(|t| t.name() == name)
        .ok_or_else(|| format!("Unknown A2A tool: {name}"))?;
    tool.handle(ctx.clone(), params).await
}

// ---------------------------------------------------------------------------
// Tool definition generation (for LLM function-calling)
// ---------------------------------------------------------------------------

/// Generate all A2A tool definitions for the LLM (currently 6).
pub fn a2a_tool_defs() -> Vec<A2aToolDef> {
    tool_table()
        .iter()
        .map(|t| A2aToolDef {
            name: t.name().to_string(),
            description: t.description().to_string(),
            parameters: t.parameters(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Register tools on a rig ToolServerHandle
// ---------------------------------------------------------------------------

/// Register all A2A tools (agent_list, agent_getCard, tasks_send, etc.) on a
/// rig [`ToolServerHandle`].
///
/// This function blocks the current thread (via `runtime.block_on`) so it must
/// be called from a non-async context (e.g., a synchronous plugin command).
///
/// # Errors
///
/// Returns an error string if any tool cannot be registered on the server.
pub fn register_tools_on_server(
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
// Re-exports for backward compat (test access)
// ---------------------------------------------------------------------------

pub use agent::handle as handle_agent_list;
pub use cancel::handle as handle_tasks_cancel;
pub use get::handle as handle_tasks_get;
pub use get_card::handle as handle_agent_get_card;
pub use list::handle as handle_tasks_list;
pub use send::handle as handle_tasks_send;
