//! Conversation turn execution using agent hooks and HookDriver bridge.
//!
//! This module provides `execute_turn` which handles a single conversation turn:
//! sending user input to the LLM, executing tool calls via hooks, and returning
//! the final response. Uses `CopilotPromptHook` + `HookDriver` to bridge async
//! events to the sync UI.

use futures::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::agent::hook::{
    driver::HookDriver, driver::PermissionResolver, prompt_hook::CopilotPromptHook,
};
use crate::agent::protocol::contracts::ProgressUi;
use crate::agent::tools::handler::McpToolRegistry;
use crate::tools::closure::ClosureRegistry;
use rig::streaming::StreamingPrompt;
use rig::tool::{ToolDyn, ToolError};
use rig::wasm_compat::WasmBoxedFuture;

/// Default max tool turns when config doesn't specify a limit.
/// Matches v1 "unlimited" semantics with a practical upper bound.
const DEFAULT_MAX_TURNS: u32 = 256;

/// Result of a successful conversation turn
#[derive(Debug)]
pub struct TurnResult {
    /// Final text response from the agent
    pub text: String,
    /// Token usage statistics
    pub usage: rig::completion::request::Usage,
    /// Complete message history (optional)
    pub messages: Option<Vec<rig::completion::Message>>,
    /// Number of tool calls executed during this turn
    pub tool_call_count: usize,
    /// Whether text deltas were emitted during streaming
    pub deltas_emitted: bool,
    /// Whether the turn was cancelled via cancel_token
    pub cancelled: bool,
    /// Last sub-call's total_tokens from the hook.
    /// This is the per-sub-call value representing actual context window usage,
    /// NOT the aggregated total across all sub-calls in this turn.
    pub last_total_tokens: u64,
}

/// Error from a conversation turn
#[derive(Debug)]
pub struct TurnError {
    /// Error message
    pub msg: String,
    /// Whether the error was due to user cancellation
    pub cancelled: bool,
    /// Messages captured at the point of cancellation (from rig's chat_history).
    /// Present only when `cancelled == true` and rig provided a chat_history.
    pub messages: Option<Vec<rig::completion::Message>>,
}

impl std::fmt::Display for TurnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.cancelled {
            write!(f, "Cancelled: {}", self.msg)
        } else {
            write!(f, "{}", self.msg)
        }
    }
}

impl std::error::Error for TurnError {}

impl From<rig::completion::PromptError> for TurnError {
    fn from(err: rig::completion::PromptError) -> Self {
        match err {
            rig::completion::PromptError::PromptCancelled {
                reason,
                chat_history,
            } => TurnError {
                msg: reason,
                cancelled: true,
                messages: Some(chat_history),
            },
            other => TurnError {
                msg: other.to_string(),
                cancelled: false,
                messages: None,
            },
        }
    }
}

impl From<rig::agent::StreamingError> for TurnError {
    fn from(e: rig::agent::StreamingError) -> Self {
        match e {
            rig::agent::StreamingError::Prompt(boxed) => match *boxed {
                rig::completion::PromptError::PromptCancelled {
                    reason,
                    chat_history,
                } => TurnError {
                    msg: reason,
                    cancelled: true,
                    messages: Some(chat_history),
                },
                other => TurnError {
                    msg: other.to_string(),
                    cancelled: false,
                    messages: None,
                },
            },
            other => TurnError {
                msg: other.to_string(),
                cancelled: false,
                messages: None,
            },
        }
    }
}

/// Context for executing a conversation turn.
pub(crate) struct TurnContext<'a, M>
where
    M: rig::completion::CompletionModel + Clone + 'static,
    M::StreamingResponse: rig::completion::request::GetTokenUsage,
{
    pub runtime: &'a tokio::runtime::Handle,
    pub model: M,
    pub prompt: String,
    pub memory: rig::memory::InMemoryConversationMemory,
    pub conversation_id: String,
    pub preamble: Option<&'a str>,
    pub max_turns: Option<u32>,
    pub tool_server_handle: rig::tool::server::ToolServerHandle,
    pub visible_tool_definitions: Vec<rig::completion::ToolDefinition>,
    pub closure_registry: &'a ClosureRegistry,
    pub mcp_registry: &'a McpToolRegistry,
}

/// Execute a conversation turn using the agent loop with hooks.
///
/// This handles a single conversation turn: sends user input through the agent,
/// which manages tool calls and LLM interactions internally. The `CopilotPromptHook`
/// intercepts events (tool calls, LLM calls) and forwards them via channels to
/// the `HookDriver`, which runs on the main thread and bridges to the sync UI.
///
/// # Architecture
///
/// ```text
/// Main thread (blocking):          Tokio runtime (async):
/// ┌─────────────────────┐         ┌──────────────────────┐
/// │ execute_turn        │ spawn → │ agent completion     │
/// │   driver.run()      │ ← ch ← │   CopilotPromptHook  │
/// │     ui.emit(...)    │         │     on_tool_call     │
/// │     perms.resolve() │         │     on_llm_start     │
/// └─────────────────────┘         └──────────────────────┘
/// ```
///
/// # Returns
///
/// `TurnResult` with the final text and usage.
///
/// # Errors
///
/// Returns `TurnError` if:
/// - The agent completion fails (LLM error, network, etc.)
/// - User cancels via UI
/// - Hook driver encounters an error
pub(crate) fn execute_turn<M, U, P>(
    ctx: TurnContext<'_, M>,
    ui: &mut U,
    permissions: &mut P,
) -> Result<TurnResult, TurnError>
where
    M: rig::completion::CompletionModel + Clone + 'static,
    M::StreamingResponse: rig::completion::request::GetTokenUsage,
    U: ProgressUi,
    P: PermissionResolver,
{
    log::info!("execute_turn: starting turn");

    // Create cancel token and hook+driver pair
    let cancel_token = CancellationToken::new();
    let (hook, mut driver) = HookDriver::new(cancel_token.clone());

    // Build the prompt message
    let user_message = rig::completion::Message::User {
        content: rig::one_or_many::OneOrMany::one(rig::completion::message::UserContent::Text(
            rig::completion::message::Text {
                text: ctx.prompt,
                additional_params: None,
            },
        )),
    };

    // Clone preamble for the 'static future
    let preamble_owned = ctx.preamble.map(|s| s.to_string());

    // Build and execute agent with hook
    let config = AgentPromptConfig {
        hook,
        cancel_token: cancel_token.clone(),
        preamble: preamble_owned,
        prompt: user_message,
        memory: ctx.memory,
        conversation_id: ctx.conversation_id,
        tool_server_handle: ctx.tool_server_handle,
        visible_tool_definitions: ctx.visible_tool_definitions,
        max_turns: ctx.max_turns,
    };

    let model = ctx.model.clone();
    let prompt_future = Box::pin(build_agent_and_stream(model, config));

    // Spawn the completion on the tokio runtime
    let prompt_handle = ctx.runtime.spawn(prompt_future);

    // Run the driver on the main thread until the completion finishes
    // The driver polls for events and handles cancellation
    driver.run_until_complete(
        ui,
        permissions,
        ctx.closure_registry,
        ctx.mcp_registry,
        &cancel_token,
    );

    // Capture tool call count and deltas flag from the driver
    let tool_call_count = driver.tool_call_count();
    let deltas_emitted = driver.deltas_emitted();
    let last_total_tokens = driver.last_total_tokens();

    log::info!(
        "execute_turn: complete, tool_calls={} deltas_emitted={}",
        tool_call_count,
        deltas_emitted
    );

    // Collect the result from the spawned task
    let join_result = ctx.runtime.block_on(prompt_handle).map_err(|e| TurnError {
        msg: format!("Agent task panicked: {}", e),
        cancelled: false,
        messages: None,
    })?;

    let response = join_result.map_err(TurnError::from)?;

    Ok(TurnResult {
        text: response.text,
        usage: response.usage,
        messages: response.messages,
        tool_call_count,
        deltas_emitted,
        cancelled: response.cancelled,
        last_total_tokens,
    })
}

/// Configuration for building and prompting an agent.
struct AgentPromptConfig {
    hook: CopilotPromptHook,
    cancel_token: CancellationToken,
    preamble: Option<String>,
    prompt: rig::completion::Message,
    memory: rig::memory::InMemoryConversationMemory,
    conversation_id: String,
    tool_server_handle: rig::tool::server::ToolServerHandle,
    visible_tool_definitions: Vec<rig::completion::ToolDefinition>,
    max_turns: Option<u32>,
}

/// Result from streaming agent execution
struct StreamingTurnResult {
    text: String,
    usage: rig::completion::request::Usage,
    messages: Option<Vec<rig::completion::Message>>,
    /// Whether the stream was cancelled via cancel_token
    cancelled: bool,
}

/// A proxy tool that forwards `call` to an existing `ToolServerHandle`
/// while providing a pre-filtered `ToolDefinition`.
///
/// This allows the agent builder to use `.tools()` (which controls what
/// the LLM sees) while dispatching execution through the original shared
/// tool server (which has all registered tool implementations).
struct FilteredToolProxy {
    tool_name: String,
    tool_definition: rig::completion::ToolDefinition,
    handle: rig::tool::server::ToolServerHandle,
}

impl ToolDyn for FilteredToolProxy {
    fn name(&self) -> String {
        self.tool_name.clone()
    }

    fn definition<'a>(
        &'a self,
        _prompt: String,
    ) -> WasmBoxedFuture<'a, rig::completion::ToolDefinition> {
        let def = self.tool_definition.clone();
        Box::pin(async move { def })
    }

    fn call<'a>(&'a self, args: String) -> WasmBoxedFuture<'a, Result<String, ToolError>> {
        Box::pin(async move {
            self.handle
                .call_tool(&self.tool_name, &args)
                .await
                .map_err(|e| ToolError::ToolCallError(Box::new(e)))
        })
    }
}

/// Build an agent with a hook and execute a multi-turn streaming prompt loop.
async fn build_agent_and_stream<M>(
    model: M,
    config: AgentPromptConfig,
) -> Result<StreamingTurnResult, rig::agent::StreamingError>
where
    M: rig::completion::CompletionModel + Clone + 'static,
    M::StreamingResponse: rig::completion::request::GetTokenUsage,
{
    let AgentPromptConfig {
        hook,
        cancel_token,
        preamble,
        prompt,
        memory,
        conversation_id,
        tool_server_handle,
        visible_tool_definitions,
        max_turns,
    } = config;

    // Create proxy tools that expose only the filtered definitions to the LLM
    // while delegating execution to the original shared tool server handle.
    let proxy_tools: Vec<Box<dyn ToolDyn>> = visible_tool_definitions
        .into_iter()
        .map(|def| {
            let proxy = FilteredToolProxy {
                tool_name: def.name.clone(),
                tool_definition: def,
                handle: tool_server_handle.clone(),
            };
            Box::new(proxy) as Box<dyn ToolDyn>
        })
        .collect();

    let mut builder = rig::agent::AgentBuilder::new(model)
        .hook(hook)
        .memory(memory.clone())
        .tools(proxy_tools);
    if let Some(ref p) = preamble {
        builder = builder.preamble(p);
    }
    let effective_max_turns = max_turns.unwrap_or(DEFAULT_MAX_TURNS);
    builder = builder.default_max_turns(effective_max_turns as usize);
    let agent = builder.build();

    let stream = agent
        .stream_prompt(prompt)
        .conversation(&conversation_id)
        .multi_turn(effective_max_turns as usize)
        .await;

    tokio::pin!(stream);

    let mut text = String::new();
    let mut usage = rig::completion::request::Usage::default();
    let mut messages: Option<Vec<rig::completion::Message>> = None;
    let mut cancelled = false;

    loop {
        tokio::select! {
            item = stream.next() => {
                match item {
                    Some(Ok(event)) => match event {
                        rig::agent::MultiTurnStreamItem::StreamAssistantItem(
                            rig::streaming::StreamedAssistantContent::Text(delta)
                        ) => {
                            text.push_str(&delta.text);
                        }
                        rig::agent::MultiTurnStreamItem::FinalResponse(fin) => {
                            text = fin.response().to_string();
                            usage = fin.usage();
                            messages = fin.history().map(|h| h.to_vec());
                        }
                        _ => {}
                    },
                    Some(Err(e)) => return Err(e),
                    None => break,
                }
            }
            _ = cancel_token.cancelled() => {
                cancelled = true;
                break;
            }
        }
    }

    Ok(StreamingTurnResult {
        text,
        usage,
        messages,
        cancelled,
    })
}

#[cfg(test)]
#[path = "turn_test.rs"]
mod turn_test;
