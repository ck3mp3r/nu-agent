//! Conversation turn execution using agent hooks and HookDriver bridge.
//!
//! This module provides `execute_turn` which handles a single conversation turn:
//! sending user input to the LLM, executing tool calls via hooks, and returning
//! the final response. Uses `CopilotPromptHook` + `HookDriver` to bridge async
//! events to the sync UI.

use std::future::Future;
use std::pin::Pin;
use tokio_util::sync::CancellationToken;

use crate::agent::hook::{
    driver::HookDriver, driver::PermissionResolver, prompt_hook::CopilotPromptHook,
};
use crate::agent::protocol::contracts::ProgressUi;
use crate::agent::tools::handler::McpToolRegistry;
use crate::providers::github_copilot::completion::CompletionModel;
use crate::providers::github_copilot::model::{Agent, ProviderVariant};
use crate::providers::github_copilot::providers::{
    AnthropicProvider, OpenAI4xProvider, OpenAI5xProvider,
};
use crate::tools::closure::ClosureRegistry;

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

/// Context for executing a conversation turn.
pub(crate) struct TurnContext<'a> {
    pub runtime: &'a tokio::runtime::Handle,
    pub agent: &'a Agent,
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
pub(crate) fn execute_turn<U: ProgressUi, P: PermissionResolver>(
    ctx: TurnContext<'_>,
    ui: &mut U,
    permissions: &mut P,
) -> Result<TurnResult, TurnError> {
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

    // Build and execute agent with hook based on variant
    // Box the futures to make them the same type
    let config = AgentPromptConfig {
        hook,
        preamble: preamble_owned,
        prompt: user_message,
        memory: ctx.memory,
        conversation_id: ctx.conversation_id,
        tool_server_handle: ctx.tool_server_handle,
        max_turns: ctx.max_turns,
    };

    let prompt_future: Pin<
        Box<
            dyn Future<Output = Result<rig::agent::PromptResponse, rig::completion::PromptError>>
                + Send,
        >,
    > = match ctx.agent.variant {
        ProviderVariant::Anthropic => {
            let model = CompletionModel::<AnthropicProvider, _>::new(
                ctx.agent.client.clone(),
                &ctx.agent.model_name,
            );
            Box::pin(build_agent_and_prompt(model, config))
        }
        ProviderVariant::OpenAI4x => {
            let model = CompletionModel::<OpenAI4xProvider, _>::new(
                ctx.agent.client.clone(),
                &ctx.agent.model_name,
            );
            Box::pin(build_agent_and_prompt(model, config))
        }
        ProviderVariant::OpenAI5x => {
            let model = CompletionModel::<OpenAI5xProvider, _>::new(
                ctx.agent.client.clone(),
                &ctx.agent.model_name,
            );
            Box::pin(build_agent_and_prompt(model, config))
        }
    };

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

    // Capture tool call count from the driver
    let tool_call_count = driver.tool_call_count();

    // Collect the result from the spawned task
    let join_result = ctx.runtime.block_on(prompt_handle).map_err(|e| TurnError {
        msg: format!("Agent task panicked: {}", e),
        cancelled: false,
    })?;

    let response = join_result.map_err(TurnError::from)?;

    Ok(TurnResult {
        text: response.output,
        usage: response.usage,
        messages: response.messages,
        tool_call_count,
    })
}

/// Configuration for building and prompting an agent.
struct AgentPromptConfig {
    hook: CopilotPromptHook,
    preamble: Option<String>,
    prompt: rig::completion::Message,
    memory: rig::memory::InMemoryConversationMemory,
    conversation_id: String,
    tool_server_handle: rig::tool::server::ToolServerHandle,
    max_turns: Option<u32>,
}

/// Build an agent with a hook and execute a multi-turn prompt loop.
async fn build_agent_and_prompt<M>(
    model: M,
    config: AgentPromptConfig,
) -> Result<rig::agent::PromptResponse, rig::completion::PromptError>
where
    M: rig::completion::CompletionModel + Clone + 'static,
{
    let AgentPromptConfig {
        hook,
        preamble,
        prompt,
        memory,
        conversation_id,
        tool_server_handle,
        max_turns,
    } = config;
    use rig::completion::Prompt;

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
    agent
        .prompt(prompt)
        .conversation(&conversation_id)
        .extended_details()
        .await
}

#[cfg(test)]
#[path = "turn_test.rs"]
mod turn_test;
