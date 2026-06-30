//! Conversation turn execution using agent hooks.
//!
//! This module provides `execute_turn` which handles a single conversation turn:
//! sending user input to the LLM, executing tool calls via hooks, and returning
//! the final response. Uses `AgentHook<P>` + a tokio MPSC channel to bridge async
//! events to the sync UI.

use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::hook::agent_hook::AgentHook;
use crate::hook::permission_resolver::AsyncPermissionResolver;
use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;
use crate::session::JournalConversationMemory;
use crate::session::repair::repair_messages;
use crate::types::{Message, Text, ToolDefinition, UserContent};
use rig::memory::ConversationMemory;
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
    pub messages: Option<Vec<Message>>,
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
    /// Number of messages in persistent storage before this turn started.
    /// Used by cancelled-turn paths (A cancelled, C) in executor.rs to slice only
    /// the new-message delta from `messages` (rig's full chat_history) so the store
    /// is not doubled when a cancelled turn follows prior session history.
    pub pre_turn_message_count: usize,
}

/// Error from a conversation turn
#[derive(Debug)]
pub struct TurnError {
    /// Error message
    pub msg: String,
    /// Whether the error was due to user cancellation
    pub cancelled: bool,
    /// Messages captured at the error site (from rig's chat_history).
    /// Present for `PromptCancelled`, `MaxTurnsError`, and `UnknownToolCall` errors.
    /// `None` for errors where rig's internal history is genuinely unrecoverable
    /// (e.g. `CompletionError`, `ToolError`).
    pub messages: Option<Vec<Message>>,
    /// History snapshot from hook, captured before the last LLM call.
    /// Empty if no `on_completion_call` fired (failure before first HTTP request).
    pub last_known_history: Vec<Message>,
    /// Number of messages in persistent storage before this turn started.
    /// Used by executor error paths to slice only the new-message delta from
    /// `last_known_history` / `messages` when deciding what to persist.
    /// Defaults to 0 in all `From` impls; overwritten by `execute_turn_with_channel`.
    pub pre_turn_message_count: usize,
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
                last_known_history: vec![],
                pre_turn_message_count: 0,
            },
            rig::completion::PromptError::MaxTurnsError {
                max_turns,
                chat_history,
                ..
            } => TurnError {
                msg: format!("Max turns ({max_turns}) exceeded"),
                cancelled: false,
                messages: Some(*chat_history),
                last_known_history: vec![],
                pre_turn_message_count: 0,
            },
            rig::completion::PromptError::UnknownToolCall {
                tool_name,
                chat_history,
                ..
            } => TurnError {
                msg: format!("Unknown tool: {tool_name}"),
                cancelled: false,
                messages: Some(*chat_history),
                last_known_history: vec![],
                pre_turn_message_count: 0,
            },
            other => TurnError {
                msg: other.to_string(),
                cancelled: false,
                messages: None,
                last_known_history: vec![],
                pre_turn_message_count: 0,
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
                    last_known_history: vec![],
                    pre_turn_message_count: 0,
                },
                rig::completion::PromptError::MaxTurnsError {
                    max_turns,
                    chat_history,
                    ..
                } => TurnError {
                    msg: format!("Max turns ({max_turns}) exceeded"),
                    cancelled: false,
                    messages: Some(*chat_history),
                    last_known_history: vec![],
                    pre_turn_message_count: 0,
                },
                rig::completion::PromptError::UnknownToolCall {
                    tool_name,
                    chat_history,
                    ..
                } => TurnError {
                    msg: format!("Unknown tool: {tool_name}"),
                    cancelled: false,
                    messages: Some(*chat_history),
                    last_known_history: vec![],
                    pre_turn_message_count: 0,
                },
                other => TurnError {
                    msg: other.to_string(),
                    cancelled: false,
                    messages: None,
                    last_known_history: vec![],
                    pre_turn_message_count: 0,
                },
            },
            other => TurnError {
                msg: other.to_string(),
                cancelled: false,
                messages: None,
                last_known_history: vec![],
                pre_turn_message_count: 0,
            },
        }
    }
}

/// Context for executing a conversation turn.
pub struct TurnConversation {
    pub memory: JournalConversationMemory,
    pub conversation_id: String,
    /// Whether this turn belongs to a persistent session.
    ///
    /// When `false` (transient), the rig `AgentBuilder` must NOT receive
    /// `.memory(...)` — rig would call `memory.append()` at turn end and write
    /// a `transient-{millis}.jsonl` file that is never reused and never cleaned
    /// up.  Omitting `.memory()` lets rig manage the turn's history in-memory
    /// within its own prompt call, which is exactly correct for a stateless
    /// one-shot invocation.
    pub has_session: bool,
}

pub struct TurnInput<'a> {
    pub prompt: String,
    pub preamble: Option<&'a str>,
    pub max_turns: Option<u32>,
}

pub struct TurnContext<'a, M>
where
    M: rig::completion::CompletionModel + Clone + 'static,
    M::StreamingResponse: rig::completion::request::GetTokenUsage,
{
    runtime: &'a tokio::runtime::Handle,
    model: M,
    conversation: TurnConversation,
    input: TurnInput<'a>,
    tool_infra: executor::ToolInfra,
    config: &'a Config,
}

impl<'a, M> TurnContext<'a, M>
where
    M: rig::completion::CompletionModel + Clone + 'static,
    M::StreamingResponse: rig::completion::request::GetTokenUsage,
{
    pub fn new(
        runtime: &'a tokio::runtime::Handle,
        model: M,
        conversation: TurnConversation,
        input: TurnInput<'a>,
        tool_infra: executor::ToolInfra,
        config: &'a Config,
    ) -> Self {
        Self {
            runtime,
            model,
            conversation,
            input,
            tool_infra,
            config,
        }
    }
}

/// Execute a conversation turn using the agent loop with hooks.
///
/// This handles a single conversation turn: sends user input through the agent,
/// which manages tool calls and LLM interactions internally. The `AgentHook<P>`
/// intercepts events (tool calls, LLM calls) and forwards them via an MPSC channel
/// to this function, which runs on the main thread and bridges to the sync UI.
///
/// # Architecture
///
/// ```text
/// Main thread (blocking):          Tokio runtime (async):
/// ┌─────────────────────┐         ┌──────────────────────┐
/// │ execute_turn        │ spawn → │ agent completion     │
/// │   ui_rx drain loop  │ ← ch ← │   AgentHook<P>       │
/// │     ui.emit(...)    │         │     on_tool_call     │
/// │     cancel_token    │         │     on_llm_start     │
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
pub fn execute_turn<M, U, P>(
    ctx: TurnContext<'_, M>,
    ui: &mut U,
    permission_resolver: P,
) -> Result<TurnResult, TurnError>
where
    M: rig::completion::CompletionModel + Clone + 'static,
    M::StreamingResponse: rig::completion::request::GetTokenUsage,
    U: ProgressUi,
    P: AsyncPermissionResolver,
{
    let (ui_tx, ui_rx) = mpsc::unbounded_channel::<UiEvent>();
    execute_turn_with_channel(ctx, ui, permission_resolver, ui_tx, ui_rx)
}

/// Execute a conversation turn with a pre-built UI event channel.
///
/// This variant is used by TUI mode where the caller creates `(ui_tx, ui_rx)` first
/// so the hook's `ui_tx` clone and the drain loop's `ui_rx` share the same channel.
/// The `InteractivePermissionResolver` does NOT own a `ui_tx` — it receives one
/// per-call from the `AgentHook` (which gets it from this same channel).
///
/// For non-interactive (TTY/policy) mode, use `execute_turn` which creates its own channel.
pub(crate) fn execute_turn_with_channel<M, U, P>(
    ctx: TurnContext<'_, M>,
    ui: &mut U,
    permission_resolver: P,
    ui_tx: mpsc::UnboundedSender<UiEvent>,
    mut ui_rx: mpsc::UnboundedReceiver<UiEvent>,
) -> Result<TurnResult, TurnError>
where
    M: rig::completion::CompletionModel + Clone + 'static,
    M::StreamingResponse: rig::completion::request::GetTokenUsage,
    U: ProgressUi,
    P: AsyncPermissionResolver,
{
    log::info!("execute_turn: starting turn");

    // Snapshot message count before this turn. Used by error paths to slice only
    // the new-message delta from last_known_history / e.messages.
    // Cache-first load: no JSONL I/O if the cache is already warm.
    let pre_turn_messages: Vec<Message> = if ctx.conversation.has_session {
        ctx.runtime
            .block_on(
                ctx.conversation
                    .memory
                    .load(&ctx.conversation.conversation_id),
            )
            .unwrap_or_default()
    } else {
        vec![]
    };
    let pre_turn_count = pre_turn_messages.len();

    // Pre-flight repair: fix structural violations before handing history to rig.
    // Ephemeral — applies for this turn only; the JSONL backing store is not written.
    let (repaired_messages, repair_issues) = repair_messages(pre_turn_messages.clone());
    if !repair_issues.is_empty() {
        log::warn!(
            "Pre-flight repair applied {} fix(es) before turn: {:?}",
            repair_issues.len(),
            repair_issues
        );
        ctx.conversation
            .memory
            .reset_context(&ctx.conversation.conversation_id, repaired_messages.clone());
    }

    // Note: on warm-cache paths `repair_messages()` is typically a no-op because
    // `JournalConversationMemory::load()` already runs the same repair pipeline on
    // every JSONL load. This second pass catches anything that slipped through at a
    // higher layer (e.g. an in-memory cache populated directly without going through
    // `load()`).
    if let Some(limit) = ctx.config.model_context_tokens {
        let estimated = token_estimate::estimate_token_count(&repaired_messages);
        let threshold =
            (limit as f32 * ctx.config.context_warning_threshold.unwrap_or(0.6)) as usize;
        if estimated >= threshold {
            let _ = ui_tx.send(UiEvent::Warning {
                message: format!(
                    "Conversation is using ~{estimated} estimated tokens \
                     (~{}% of the {limit}-token context window). \
                     Consider running 'agent session compact' before the next turn.",
                    (estimated * 100) / limit,
                ),
            });
        }
    }

    // Create cancel token — use an externally supplied token if the UI provides one
    // (e.g. MockUi::with_external_cancel() in tests, or a real UI that wants to cancel
    // from outside the drain loop). Falls back to a fresh token when none is provided.
    let cancel_token = ui.external_cancel_token().unwrap_or_default();

    // Build the hook using AgentHook<P> — no HookDriver needed
    let hook = AgentHook::new(
        cancel_token.clone(),
        ui_tx,
        permission_resolver,
        ctx.tool_infra.closure_registry.clone(),
        ctx.tool_infra.mcp_registry.clone(),
    );

    // Clone the Arc BEFORE hook moves into config so we can read history after a
    // CompletionError (when rig does not provide chat_history in TurnError::messages).
    let last_known_history_arc = hook.last_known_history();

    // Build the prompt message
    let user_message = Message::User {
        content: rig::one_or_many::OneOrMany::one(UserContent::Text(Text {
            text: ctx.input.prompt,
            additional_params: None,
        })),
    };

    // Clone preamble for the 'static future
    let preamble_owned = ctx.input.preamble.map(|s| s.to_string());

    // Build and execute agent with hook
    let config = AgentPromptConfig {
        hook,
        preamble: preamble_owned,
        prompt: user_message,
        memory: ctx.conversation.memory,
        conversation_id: ctx.conversation.conversation_id,
        has_session: ctx.conversation.has_session,
        tool_server_handle: ctx.tool_infra.tool_server_handle,
        visible_tool_definitions: ctx.tool_infra.visible_tool_definitions,
        max_turns: ctx.input.max_turns,
        cancel_token: cancel_token.clone(),
    };

    let model = ctx.model.clone();
    let prompt_future = Box::pin(build_agent_and_stream(model, config));

    // Spawn the completion on the tokio runtime.
    // Cancellation note: cancel_token fires Terminate on the next hook entry (on_completion_call,
    // on_text_delta, or on_tool_call), causing rig to yield PromptCancelled { chat_history }.
    // If the HTTP request hangs before any hook fires (e.g. dead network), the stream blocks
    // until the provider's own HTTP client timeout. Ensure timeouts are configured on the client.
    let prompt_handle = ctx.runtime.spawn(prompt_future);

    // Main-thread drain loop: forward UiEvents from the hook to the UI,
    // and propagate cancel requests from the UI to the cancel token.
    loop {
        if ui.take_cancel_requested() {
            cancel_token.cancel();
        }
        match ui_rx.try_recv() {
            Ok(event) => ui.emit(&event),
            Err(mpsc::error::TryRecvError::Empty) => {
                ui.emit(&UiEvent::Tick);
                std::thread::sleep(std::time::Duration::from_millis(16));
            }
            Err(mpsc::error::TryRecvError::Disconnected) => break,
        }
    }
    ui.flush();

    log::info!("execute_turn: ui_rx drained, joining spawn handle");

    // Collect the result from the spawned task
    let join_result = ctx.runtime.block_on(prompt_handle).map_err(|e| TurnError {
        msg: format!("Agent task panicked: {}", e),
        cancelled: false,
        messages: None,
        last_known_history: vec![],
        pre_turn_message_count: 0,
    })?;

    let response = join_result.map_err(|e| {
        let mut err = TurnError::from(e);
        err.last_known_history = last_known_history_arc.lock().unwrap().clone();
        err.pre_turn_message_count = pre_turn_count;
        err
    })?;

    log::info!(
        "execute_turn: complete, tool_calls={} deltas_emitted={}",
        response.tool_call_count,
        response.deltas_emitted,
    );

    Ok(TurnResult {
        text: response.text,
        usage: response.usage,
        messages: response.messages,
        tool_call_count: response.tool_call_count,
        deltas_emitted: response.deltas_emitted,
        cancelled: response.cancelled,
        last_total_tokens: response.last_total_tokens,
        pre_turn_message_count: pre_turn_count,
    })
}

/// Configuration for building and prompting an agent.
struct AgentPromptConfig<P: AsyncPermissionResolver> {
    hook: AgentHook<P>,
    preamble: Option<String>,
    prompt: Message,
    memory: JournalConversationMemory,
    conversation_id: String,
    /// Whether this turn belongs to a persistent session.
    ///
    /// When `false`, `.memory()` is NOT attached to the rig `AgentBuilder` so
    /// rig never calls `memory.append()` and no JSONL file is written to disk.
    has_session: bool,
    tool_server_handle: rig::tool::server::ToolServerHandle,
    visible_tool_definitions: Vec<ToolDefinition>,
    max_turns: Option<u32>,
    cancel_token: CancellationToken,
}

/// Result from streaming agent execution
struct StreamingTurnResult {
    text: String,
    usage: rig::completion::request::Usage,
    messages: Option<Vec<Message>>,
    /// Whether the stream was cancelled via cancel_token
    cancelled: bool,
    /// Number of complete tool calls seen in the stream
    tool_call_count: usize,
    /// Whether any text deltas were emitted (i.e. streaming was active)
    deltas_emitted: bool,
    /// Total tokens from the most recent CompletionCall event
    last_total_tokens: u64,
}

/// A proxy tool that forwards `call` to an existing `ToolServerHandle`
/// while providing a pre-filtered `ToolDefinition`.
///
/// This allows the agent builder to use `.tools()` (which controls what
/// the LLM sees) while dispatching execution through the original shared
/// tool server (which has all registered tool implementations).
struct FilteredToolProxy {
    tool_name: String,
    tool_definition: ToolDefinition,
    handle: rig::tool::server::ToolServerHandle,
    cancel_token: CancellationToken,
}

impl ToolDyn for FilteredToolProxy {
    fn name(&self) -> String {
        self.tool_name.clone()
    }

    fn definition<'a>(&'a self, _prompt: String) -> WasmBoxedFuture<'a, ToolDefinition> {
        let def = self.tool_definition.clone();
        Box::pin(async move { def })
    }

    fn call<'a>(&'a self, args: String) -> WasmBoxedFuture<'a, Result<String, ToolError>> {
        let cancel_token = self.cancel_token.clone();
        Box::pin(async move {
            tokio::select! {
                result = self.handle.call_tool(&self.tool_name, &args) => {
                    result.map_err(|e| ToolError::ToolCallError(Box::new(e)))
                }
                _ = cancel_token.cancelled() => {
                    Err(ToolError::ToolCallError(
                        "tool call cancelled".to_string().into()
                    ))
                }
            }
        })
    }
}

/// Build an agent with a hook and execute a multi-turn streaming prompt loop.
async fn build_agent_and_stream<M, P>(
    model: M,
    config: AgentPromptConfig<P>,
) -> Result<StreamingTurnResult, rig::agent::StreamingError>
where
    M: rig::completion::CompletionModel + Clone + 'static,
    M::StreamingResponse: rig::completion::request::GetTokenUsage,
    P: AsyncPermissionResolver,
{
    let AgentPromptConfig {
        hook,
        preamble,
        prompt,
        memory,
        conversation_id,
        has_session,
        tool_server_handle,
        visible_tool_definitions,
        max_turns,
        cancel_token,
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
                cancel_token: cancel_token.clone(),
            };
            Box::new(proxy) as Box<dyn ToolDyn>
        })
        .collect();

    // Only attach memory when this is a persistent session.
    //
    // For transient (no-session) invocations, omitting `.memory()` prevents rig
    // from calling `memory.append()` at turn end — which would otherwise write a
    // `transient-{millis}.jsonl` file to disk that is never reused and never
    // cleaned up.  When memory is absent rig manages the turn's history in-memory
    // within its own prompt call, which is exactly correct for a stateless
    // one-shot invocation.
    let mut builder = if has_session {
        rig::agent::AgentBuilder::new(model)
            .hook(hook)
            .memory(memory)
            .tools(proxy_tools)
    } else {
        rig::agent::AgentBuilder::new(model)
            .hook(hook)
            .tools(proxy_tools)
    };
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
    let mut messages: Option<Vec<Message>> = None;
    let mut tool_call_count: usize = 0;
    let mut last_total_tokens: u64 = 0;
    let mut cancelled = false;
    let mut deltas_emitted = false;

    loop {
        let item = tokio::select! {
            biased;
            item = stream.next() => item,
            _ = cancel_token.cancelled() => {
                cancelled = true;
                break;
            }
        };
        match item {
            Some(Ok(event)) => match event {
                // --- STREAMED ASSISTANT CONTENT ---
                rig::agent::MultiTurnStreamItem::StreamAssistantItem(content) => {
                    match content {
                        // TEXT DELTA
                        rig::streaming::StreamedAssistantContent::Text(delta) => {
                            text.push_str(&delta.text);
                            deltas_emitted = true;
                        }
                        // TOOL CALL (complete, post-assembly)
                        // Hook's on_tool_call has already resolved. The hook already
                        // emitted ToolStart + permission events.
                        rig::streaming::StreamedAssistantContent::ToolCall { .. } => {
                            tool_call_count += 1;
                        }
                        // TOOL CALL DELTA (streaming args)
                        // Hook's on_tool_call_delta already fired — no-op here.
                        rig::streaming::StreamedAssistantContent::ToolCallDelta { .. } => {}
                        // REASONING block — ignore for now
                        rig::streaming::StreamedAssistantContent::Reasoning(_) => {}
                        // REASONING DELTA — ignore for now
                        rig::streaming::StreamedAssistantContent::ReasoningDelta { .. } => {}
                        // Raw provider final response object — not needed here
                        rig::streaming::StreamedAssistantContent::Final(_) => {}
                    }
                }

                // --- TOOL RESULT (user content fed back to model) ---
                // The hook's on_tool_result already fired and emitted ToolEnd.
                rig::agent::MultiTurnStreamItem::StreamUserItem(
                    rig::streaming::StreamedUserContent::ToolResult { .. },
                ) => {}

                // --- PER-SUBCALL USAGE ---
                rig::agent::MultiTurnStreamItem::CompletionCall(call) => {
                    last_total_tokens = call.usage.total_tokens;
                }

                // --- FINAL RESPONSE ---
                rig::agent::MultiTurnStreamItem::FinalResponse(fin) => {
                    text = fin.response().to_string();
                    usage = fin.usage();
                    messages = fin.history().map(|h| h.to_vec());
                }

                // MultiTurnStreamItem is #[non_exhaustive] — required wildcard arm.
                // Future rig versions may add new variants; we ignore them here.
                _ => {}
            },
            Some(Err(e)) => {
                // Check whether rig cancelled the agent loop via the hook's Terminate action.
                match e {
                    rig::agent::StreamingError::Prompt(boxed) => match *boxed {
                        rig::completion::PromptError::PromptCancelled { chat_history, .. } => {
                            messages = Some(chat_history);
                            cancelled = true;
                            break;
                        }
                        other => {
                            return Err(rig::agent::StreamingError::Prompt(Box::new(other)));
                        }
                    },
                    other => return Err(other),
                }
            }
            None => break,
        }
    }

    Ok(StreamingTurnResult {
        text,
        usage,
        messages,
        cancelled,
        tool_call_count,
        deltas_emitted,
        last_total_tokens,
    })
}

pub mod executor;

pub(crate) mod token_estimate;

#[cfg(test)]
mod test;

#[cfg(test)]
mod cancel;
