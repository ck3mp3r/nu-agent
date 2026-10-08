use serde_json::Value;

use super::adapter::{A2aToolAdapter, A2aToolContext, A2aToolDef};

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
                "Send a task to another agent over A2A. The task runs asynchronously: this tool returns immediately with a taskId, and the result arrives later as a new conversation turn. Do NOT poll tasks_get or tasks_list for completion.\n\nSession control: omit contextId to start a fresh session and lose all prior context; ALWAYS include contextId from a prior response to resume that session.\n\nReturns {taskId, contextId, status, message}. Keep taskId and contextId for follow-up sends. Check if the agent is busy first with tasks_list(status=\"TASK_STATE_WORKING\", target=name)."
            }
            Tool::Get => {
                "Fetch a task's current state and artifacts from a remote agent. Use after a completion notification to read the full result. Requires taskId (from tasks_send) and target (agent name from agent_list).\n\nReturns {taskId, state, artifacts}. The artifacts array holds the result content; read the text parts."
            }
            Tool::List => {
                "List tasks, optionally filtered by status. With target, lists tasks from that remote agent. Without target, lists tasks in the local store (tasks received by this agent).\n\nFilter by any TASK_STATE_* value (e.g. TASK_STATE_WORKING, TASK_STATE_COMPLETED). Use TASK_STATE_WORKING with target to check if an agent is busy.\n\nReturns {tasks: [...]}. Do NOT poll repeatedly for a task you just sent — completion arrives as a new turn."
            }
            Tool::Cancel => {
                "Cancel a running task on a remote agent. Use when a task is no longer needed or runs too long. Both taskId (from tasks_send) and target (agent name from agent_list) are required.\n\nReturns {taskId, state}. A successful cancel yields TASK_STATE_CANCELED. Canceling an already-terminal task fails."
            }
            Tool::AgentList => {
                "List all connected A2A agents on the network mesh. Call this first to discover which agents you can delegate tasks to. Excludes yourself.\n\nReturns {agents: [{name, url, description, skills}]}. Use name as the target parameter for tasks_send, tasks_get, and tasks_cancel."
            }
            Tool::GetCard => {
                "Fetch the full A2A agent card for one agent. The card holds capabilities (streaming, pushNotifications, stateful), skills with input and output modes, and security requirements. Use it to check detailed capabilities before delegating complex tasks; agent_list already gives name, description, and skill names.\n\nReturns the card JSON, or {name, error} when the fetch fails."
            }
        }
    }

    pub fn parameters(&self) -> Value {
        // Same as current register.rs parameters() function
        match self {
            Tool::Send => serde_json::json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string", "description": "Name of the target agent, from agent_list"},
                    "text": {"type": "string", "description": "Task text to send to the target agent"},
                    "contextId": {"type": "string", "description": "Optional: include to resume an existing session on the target agent. Omit to start a fresh session and lose all prior context."}
                },
                "required": ["target", "text"]
            }),
            Tool::Get => serde_json::json!({
                "type": "object",
                "properties": {
                    "taskId": {"type": "string", "description": "Task ID returned by tasks_send"},
                    "target": {"type": "string", "description": "Name of the agent that owns the task, from agent_list"}
                },
                "required": ["taskId", "target"]
            }),
            Tool::List => serde_json::json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string", "description": "Optional: name of a remote agent, from agent_list. Omit to list tasks in the local store."},
                    "status": {"type": "string", "description": "Optional: filter by task state. Filter by any TASK_STATE_* value (e.g. TASK_STATE_WORKING, TASK_STATE_COMPLETED)."}
                }
            }),
            Tool::Cancel => serde_json::json!({
                "type": "object",
                "properties": {
                    "taskId": {"type": "string", "description": "Task ID returned by tasks_send"},
                    "target": {"type": "string", "description": "Name of the agent that owns the task, from agent_list"}
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
                    "name": {"type": "string", "description": "Name of the agent, from agent_list"}
                },
                "required": ["name"]
            }),
        }
    }

    pub async fn handle(&self, ctx: A2aToolContext, params: Value) -> ToolResult {
        match self {
            Tool::Send => super::send::handle(ctx, params).await,
            Tool::Get => super::get::handle(ctx, params).await,
            Tool::List => super::list::handle(ctx, params).await,
            Tool::Cancel => super::cancel::handle(ctx, params).await,
            Tool::AgentList => super::agent::handle(ctx, params).await,
            Tool::GetCard => super::get_card::handle(ctx, params).await,
        }
    }
}

// ---------------------------------------------------------------------------
// Tool table (lazy)
// ---------------------------------------------------------------------------

fn tool_table() -> &'static Vec<Tool> {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Vec<Tool>> = OnceLock::new();
    TABLE.get_or_init(super::register::register_a2a_tools)
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

pub(crate) async fn handle_dispatch(name: &str, ctx: &A2aToolContext, params: Value) -> ToolResult {
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
/// rig [`ToolServerHandle`] using DynamicTool.
///
/// # Errors
///
/// Returns an error string if any tool cannot be registered on the server.
pub async fn register_tools_on_server(
    tool_server_handle: &rig::tool::server::ToolServerHandle,
    ctx: A2aToolContext,
) -> Result<(), String> {
    for def in a2a_tool_defs() {
        let adapter = A2aToolAdapter::new(def, ctx.clone());
        tool_server_handle.add_dynamic_tool(adapter.into_dynamic_tool());
    }
    Ok(())
}
