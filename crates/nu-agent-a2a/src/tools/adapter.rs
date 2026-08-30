use std::sync::Arc;

use rig::tool::{DynamicTool, ToolExecutionError, ToolOutput};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::*;

use super::registry::handle_dispatch;

// ---------------------------------------------------------------------------
// A2aToolAdapter — bridges A2A tools into rig's DynamicTool system
// ---------------------------------------------------------------------------

/// Adapter that wraps an A2A tool for rig's DynamicTool registration system.
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

    /// Convert this adapter into a DynamicTool for registration with rig.
    pub fn into_dynamic_tool(self) -> DynamicTool {
        let name = self.tool_def.name.clone();
        let description = self.tool_def.description.clone();
        let parameters = self.tool_def.parameters.clone();
        let ctx = self.ctx;

        DynamicTool::new(
            name.clone(),
            description,
            parameters,
            move |_context, args| {
                let ctx = ctx.clone();
                let name = name.clone();
                Box::pin(async move {
                    let result = handle_dispatch(&name, &ctx, args)
                        .await
                        .map_err(ToolExecutionError::provider)?;

                    serde_json::to_string(&result)
                        .map(ToolOutput::text)
                        .map_err(|e| {
                            ToolExecutionError::other(format!(
                                "Failed to serialize A2A tool result: {e}"
                            ))
                        })
                })
            },
        )
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
