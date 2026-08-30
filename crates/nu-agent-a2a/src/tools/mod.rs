mod adapter;
mod agent;
mod cancel;
mod get;
mod get_card;
mod list;
mod register;
mod registry;
mod send;

#[cfg(test)]
mod test;

pub use adapter::{A2aToolAdapter, A2aToolContext, A2aToolDef};
pub use registry::{Tool, ToolResult, a2a_tool_defs, register_tools_on_server};

// ---------------------------------------------------------------------------
// Re-exports for backward compat (test access)
// ---------------------------------------------------------------------------

pub use agent::handle as handle_agent_list;
pub use cancel::handle as handle_tasks_cancel;
pub use get::handle as handle_tasks_get;
pub use get_card::handle as handle_agent_get_card;
pub use list::handle as handle_tasks_list;
pub use send::handle as handle_tasks_send;
