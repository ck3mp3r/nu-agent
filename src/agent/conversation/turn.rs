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

/// Default max tool turns when config doesn't specify a limit.
/// Matches v1 "unlimited" semantics with a practical upper bound.
const DEFAULT_MAX_TURNS: u32 = 64;

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
}

/// Error from a conversation turn
#[derive(Debug)]
pub struct TurnError {
    /// Error message
    pub msg: String,
    /// Whether the error was due to user cancellation
    pub cancelled: bool,
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
            rig::completion::PromptError::PromptCancelled { reason, .. } => TurnError {
                msg: reason,
                cancelled: true,
            },
            other => TurnError {
                msg: other.to_string(),
                cancelled: false,
            },
        }
    }
}

impl From<rig::agent::StreamingError> for TurnError {
    fn from(e: rig::agent::StreamingError) -> Self {
        match e {
            rig::agent::StreamingError::Prompt(boxed) => match *boxed {
                rig::completion::PromptError::PromptCancelled { reason, .. } => TurnError {
                    msg: reason,
                    cancelled: true,
                },
                other => TurnError {
                    msg: other.to_string(),
                    cancelled: false,
                },
            },
            other => TurnError {
                msg: other.to_string(),
                cancelled: false,
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
            rig::completion::message::Text { text: ctx.prompt },
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

    log::info!(
        "execute_turn: complete, tool_calls={} deltas_emitted={}",
        tool_call_count,
        deltas_emitted
    );

    // Collect the result from the spawned task
    let join_result = ctx.runtime.block_on(prompt_handle).map_err(|e| TurnError {
        msg: format!("Agent task panicked: {}", e),
        cancelled: false,
    })?;

    let response = join_result.map_err(TurnError::from)?;

    Ok(TurnResult {
        text: response.text,
        usage: response.usage,
        messages: response.messages,
        tool_call_count,
        deltas_emitted,
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
    max_turns: Option<u32>,
}

/// Result from streaming agent execution
struct StreamingTurnResult {
    text: String,
    usage: rig::completion::request::Usage,
    messages: Option<Vec<rig::completion::Message>>,
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
        max_turns,
    } = config;

    let mut builder = rig::agent::AgentBuilder::new(model)
        .hook(hook)
        .memory(memory.clone())
        .tool_server_handle(tool_server_handle);
    if let Some(ref p) = preamble {
        builder = builder.preamble(p);
    }
    let effective_max_turns = max_turns.unwrap_or(DEFAULT_MAX_TURNS);
    builder = builder.default_max_turns(effective_max_turns as usize);
    let agent = builder.build();

    let stream = agent
        .stream_prompt(prompt)
        .conversation(&conversation_id)
        .with_history(Vec::<rig::completion::Message>::new())
        .multi_turn(effective_max_turns as usize)
        .await;

    tokio::pin!(stream);

    let mut text = String::new();
    let mut usage = rig::completion::request::Usage::default();
    let mut messages: Option<Vec<rig::completion::Message>> = None;

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
                break;
            }
        }
    }

    Ok(StreamingTurnResult {
        text,
        usage,
        messages,
    })
}

#[cfg(test)]
#[path = "turn_test.rs"]
mod turn_test;
