//! Conversation turn execution using agent hooks.
//!
//! This module provides `execute_turn` which handles a single conversation turn:
//! sending user input to the LLM, executing tool calls via hooks, and returning
//! the final response. Permission and lifecycle events flow to consumers through
//! the shared `Bus`; core never threads a `ProgressUi` through the turn.

use futures::StreamExt;
use std::sync::Arc;

use crate::bus::{Bus, WarningEvent};
use crate::config::defaults;
use crate::conversation::state::memory::MemoryOf;
use crate::hook::agent_hook::HookState;
use crate::hook::chain::HookChain;
use crate::hook::permission_resolver::AsyncPermissionResolver;
use crate::session::SessionStore;
use crate::session::repair::repair_messages;
use crate::types::{Message, Text, ToolDefinition, UserContent};
use rig::memory::ConversationMemory;
use rig::streaming::StreamingPrompt;
use rig::tool::DynamicTool;

use super::context::TurnContext;
use super::error;
use super::error::TurnError;
use super::executor;
use super::proxy::FilteredToolProxy;
use super::token_estimate;

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
    /// The cancellation reason when the turn was cancelled by a hook stop
    /// (e.g. doom-loop stop). `None` for bus-cancel and normal completion.
    pub cancel_reason: Option<String>,
    /// Last sub-call's total_tokens from the hook.
    /// This is the per-sub-call value representing actual context window usage,
    /// NOT the aggregated total across all sub-calls in this turn.
    pub last_total_tokens: u64,
    /// Number of messages in persistent storage before this turn started.
    /// Used by cancelled-turn paths (A cancelled, C) in executor.rs to slice only
    /// the new-message delta from `messages` (rig's full chat_history) so the store
    /// is not doubled when a cancelled turn follows prior session history.
    pub pre_turn_message_count: usize,
    /// History snapshot from hook, captured before the last LLM call.
    /// Populated from `HookChain::last_known_history()` Arc after the turn completes.
    /// Used by executor's Path B cancel fallback when `messages` is `None` (tokio::select
    /// cancelled before rig yielded PromptCancelled) to persist completed tool calls
    /// instead of synthesizing a minimal `[user(prompt)]` placeholder.
    pub last_known_history: Vec<Message>,
}

/// Execute a conversation turn using the agent loop with hooks.
///
/// This handles a single conversation turn: sends user input through the agent,
/// which manages tool calls and LLM interactions internally. The `HookChain<P>`
/// intercepts events (tool calls, LLM calls) and forwards them to the shared
/// `Bus`. Cancellation is delivered via `bus.cancel()`, so no per-turn UI
/// channel or drain loop is needed; the future completes when the agent
/// finishes.
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
pub(crate) async fn execute_turn<S, P>(
    ctx: TurnContext<'_, S>,
    permission_resolver: P,
) -> Result<TurnResult, (TurnError, error::TurnContext)>
where
    S: SessionStore + Clone + Send + Sync + 'static,
    P: AsyncPermissionResolver,
{
    log::info!("execute_turn: starting turn");

    // Snapshot message count before this turn. Used by error paths to slice only
    // the new-message delta from the error variant's history fields.
    // Cache-first load: no JSONL I/O if the cache is already warm.
    let pre_turn_messages: Vec<Message> = if ctx.conversation.has_session {
        ctx.conversation
            .memory
            .load(&ctx.conversation.conversation_id)
            .await
            .unwrap_or_default()
    } else {
        vec![]
    };
    let pre_turn_count = pre_turn_messages.len();
    log::debug!(
        "execute_turn: pre_turn_count={pre_turn_count} session={}",
        ctx.conversation.has_session
    );

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
    // Build the hook using HookChain<P> — no HookDriver needed.
    // Cancellation is driven through the shared bus's cancel channel.
    let bus = ctx.tool_infra.bus.clone();
    let shared_model = Arc::clone(&ctx.conversation.shared_model);
    let hook = HookChain::new(
        bus.clone(),
        permission_resolver,
        ctx.tool_infra.closure_registry.clone(),
        ctx.tool_infra.mcp_registry.clone(),
        ctx.config.max_tool_calls_per_subturn,
        HookState {
            circuit_breaker: ctx.tool_infra.circuit_breaker.clone(),
            doom_state: ctx.tool_infra.doom_state.clone(),
            shared_model,
            memory: Arc::clone(&ctx.conversation.memory),
            conversation_id: ctx.conversation.conversation_id.clone(),
            compaction: ctx.conversation.compaction.clone(),
            last_total_tokens: ctx.tool_infra.last_total_tokens.clone(),
        },
    );

    // Clone the Arc BEFORE hook moves into config so we can read history after a
    // CompletionError (when rig does not provide chat_history in TurnError::messages).
    let last_known_history_arc = hook.last_known_history();

    // Build the prompt message
    let user_message = Message::User {
        content: vec![UserContent::Text(Text {
            text: ctx.input.prompt,
            additional_params: None,
        })],
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
        shared_model: ctx.conversation.shared_model,
        tool_server_handle: ctx.tool_infra.tool_server_handle,
        visible_tool_definitions: ctx.tool_infra.visible_tool_definitions,
        max_turns: ctx.input.max_turns,
        bus: bus.clone(),
        additional_params: ctx.config.additional_params.clone(),
        temperature: ctx.config.temperature,
        max_tokens: ctx
            .config
            .max_tokens
            .or(ctx.config.max_output_tokens)
            .map(|t| t as u64),
    };

    let prompt_future = Box::pin(build_agent_and_stream(config));

    // Publish a context-window warning on the warning bus channel. Warnings are
    // read from the bus directly by the TUI render loop and the TTY stderr
    // subscriber.
    if let Some(limit) = ctx.config.model_context_tokens {
        let estimated = token_estimate::estimate_token_count(&repaired_messages);
        let threshold = (limit as f32
            * ctx
                .config
                .context_warning_threshold
                .unwrap_or(defaults::CONTEXT_WARNING_THRESHOLD)) as usize;
        log::debug!("execute_turn: token_estimate={estimated} threshold={threshold} limit={limit}");
        if estimated >= threshold {
            let _ = bus
                .warning()
                .send(WarningEvent::Message {
                    message: format!(
                        "Conversation is using ~{estimated} estimated tokens \
                     (~{}% of the {limit}-token context window). \
                     Consider running 'agent session compact' before the next turn.",
                        (estimated * 100) / limit,
                    ),
                })
                .await;
        }
    }

    // Spawn the completion on the current task.
    // Cancellation note: publishing CancelEvent on the bus fires Terminate on the next
    // hook entry (on_completion_call, on_text_delta, or on_tool_call), causing rig to
    // yield PromptCancelled { chat_history }. If the HTTP request hangs before any hook
    // fires (e.g. dead network), the stream blocks until the provider's own HTTP client
    // timeout. Ensure timeouts are configured on the client.
    let prompt_handle = tokio::spawn(prompt_future);

    // Await the completion. Cancellation is delivered via `bus.cancel()`: the
    // hook and tool proxies subscribe to that channel and fire `Terminate` /
    // return a `Cancelled` error, so no per-turn UI channel or drain loop is
    // needed here.
    log::info!("execute_turn: awaiting agent completion");

    // Collect the result from the spawned task
    let join_result = prompt_handle.await.map_err(|e| {
        log::error!("Agent task panicked: {e}");
        let err = TurnError::CompletionFailed {
            msg: format!("Agent task panicked: {e}"),
            kind: executor::CompletionErrorKind::Unknown,
        };
        let ctx = error::TurnContext {
            last_known_history: last_known_history_arc
                .lock()
                .expect("mutex poisoned")
                .clone(),
            pre_turn_message_count: pre_turn_count,
        };
        (err, ctx)
    })?;

    let response = join_result.map_err(|e| {
        let err = TurnError::from(e);
        let ctx = error::TurnContext {
            last_known_history: last_known_history_arc
                .lock()
                .expect("mutex poisoned")
                .clone(),
            pre_turn_message_count: pre_turn_count,
        };
        (err, ctx)
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
        cancel_reason: response.cancel_reason,
        last_total_tokens: response.last_total_tokens,
        pre_turn_message_count: pre_turn_count,
        last_known_history: last_known_history_arc.lock().unwrap().clone(),
    })
}

/// Configuration for building and prompting an agent.
struct AgentPromptConfig<S: SessionStore + Clone + Send + Sync, P: AsyncPermissionResolver> {
    hook: HookChain<P, S>,
    preamble: Option<String>,
    prompt: Message,
    memory: MemoryOf<S>,
    conversation_id: String,
    /// Whether this turn belongs to a persistent session.
    ///
    /// When `false`, `.memory()` is NOT attached to the rig `AgentBuilder` so
    /// rig never calls `memory.append()` and no JSONL file is written to disk.
    has_session: bool,
    /// Shared runtime model handle. The agent is built from this handle so the
    /// hook's `on_model_select` (which routes to the same shared value) stays in
    /// sync with the model the agent was constructed from. It is constructed
    /// eagerly at startup.
    shared_model: std::sync::Arc<std::sync::Mutex<rig::agent::ModelHandle>>,
    tool_server_handle: rig::tool::server::ToolServerHandle,
    visible_tool_definitions: Vec<ToolDefinition>,
    max_turns: Option<u32>,
    bus: Bus,
    additional_params: Option<serde_json::Value>,
    temperature: Option<f64>,
    max_tokens: Option<u64>,
}

/// Result from streaming agent execution
struct StreamingTurnResult {
    text: String,
    usage: rig::completion::request::Usage,
    messages: Option<Vec<Message>>,
    /// Whether the stream was cancelled via the bus cancel channel.
    cancelled: bool,
    /// The cancellation reason when the stream was cancelled by a hook stop
    /// (e.g. doom-loop stop). `None` for bus-cancel and normal completion.
    cancel_reason: Option<String>,
    /// Number of complete tool calls seen in the stream
    tool_call_count: usize,
    /// Whether any text deltas were emitted (i.e. streaming was active)
    deltas_emitted: bool,
    /// Total tokens from the most recent CompletionCall event
    last_total_tokens: u64,
}

/// Build an agent with a hook and execute a multi-turn streaming prompt loop.
async fn build_agent_and_stream<S, P>(
    config: AgentPromptConfig<S, P>,
) -> Result<StreamingTurnResult, rig::agent::StreamingError>
where
    S: SessionStore + Clone + Send + Sync + 'static,
    P: AsyncPermissionResolver,
{
    let AgentPromptConfig {
        hook,
        preamble,
        prompt,
        memory,
        conversation_id,
        has_session,
        shared_model,
        tool_server_handle,
        visible_tool_definitions,
        max_turns,
        bus,
        additional_params,
        temperature,
        max_tokens,
    } = config;
    // Create proxy tools that expose only the filtered definitions to the LLM
    // while delegating execution to the original shared tool server handle.
    let proxy_tools: Vec<DynamicTool> = visible_tool_definitions
        .into_iter()
        .map(|def| {
            let proxy = FilteredToolProxy {
                tool_name: def.name.clone(),
                tool_definition: def,
                handle: tool_server_handle.clone(),
                bus: bus.clone(),
            };
            proxy.into_dynamic_tool()
        })
        .collect();

    // Build the agent from the shared model handle. The agent's model is erased
    // once into a `ModelHandle`; the hook's `on_model_select` routes each turn to
    // the same shared handle's current value, so `switch_model()` updates both
    // the agent's model and the per-turn routing in one place. The handle is
    // constructed eagerly at startup, so it is always present here.
    let model_handle = shared_model.lock().expect("model mutex poisoned").clone();

    // Only attach memory when this is a persistent session.
    //
    // For transient (no-session) invocations, omitting `.memory()` prevents rig
    // from calling `memory.append()` at turn end — which would otherwise write a
    // `transient-{millis}.jsonl` file to disk that is never reused and never
    // cleaned up.  When memory is absent rig manages the turn's history in-memory
    // within its own prompt call, which is exactly correct for a stateless
    // one-shot invocation.
    let mut builder = if has_session {
        rig::agent::AgentBuilder::from_model_handle(model_handle)
            .add_hook(hook)
            .memory(memory)
            .dynamic_tools(proxy_tools)
    } else {
        rig::agent::AgentBuilder::from_model_handle(model_handle)
            .add_hook(hook)
            .dynamic_tools(proxy_tools)
    };
    if let Some(p) = preamble {
        builder = builder.preamble(&p);
    }
    let effective_max_turns = max_turns.unwrap_or(DEFAULT_MAX_TURNS);
    builder = builder.default_max_turns(effective_max_turns as usize);
    if let Some(params) = additional_params {
        builder = builder.additional_params(params);
    }
    if let Some(t) = temperature {
        builder = builder.temperature(t);
    }
    if let Some(m) = max_tokens {
        builder = builder.max_tokens(m);
    }
    let agent = builder.build();

    let stream = agent
        .stream_prompt(prompt)
        .conversation(&conversation_id)
        .max_turns(effective_max_turns as usize)
        .max_invalid_tool_call_retries(3)
        .await;

    tokio::pin!(stream);

    let mut text = String::new();
    let mut usage = rig::completion::request::Usage::default();
    let mut messages: Option<Vec<Message>> = None;
    let mut tool_call_count: usize = 0;
    let mut last_total_tokens: u64 = 0;
    let mut cancelled = false;
    let mut cancel_reason: Option<String> = None;
    let mut deltas_emitted = false;
    let mut cancel_rx = bus.cancel().subscribe();

    loop {
        let item = tokio::select! {
            biased;
            item = stream.next() => item,
            Ok(_) = cancel_rx.recv() => {
                log::trace!("Stream cancelled via bus cancel channel");
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
                        // emitted ToolStarted + permission events.
                        rig::streaming::StreamedAssistantContent::ToolCall { .. } => {
                            tool_call_count += 1;
                        }
                        // TOOL CALL DELTA (streaming args)
                        // Hook's on_tool_call_delta already fired — no-op here.
                        rig::streaming::StreamedAssistantContent::ToolCallDelta { .. } => {}
                        // REASONING block — ignore for now
                        rig::streaming::StreamedAssistantContent::Reasoning { .. } => {}
                        // REASONING DELTA — ignore for now
                        rig::streaming::StreamedAssistantContent::ReasoningDelta { .. } => {}
                        // Raw provider final response object — not needed here
                        rig::streaming::StreamedAssistantContent::Final(_) => {}
                        // Unknown provider-specific content — ignore
                        rig::streaming::StreamedAssistantContent::Unknown(_) => {}
                    }
                }

                // --- TOOL RESULT (user content fed back to model) ---
                // The hook's on_tool_result already fired and emitted ToolCompleted.
                rig::agent::MultiTurnStreamItem::StreamUserItem(
                    rig::streaming::StreamedUserContent::ToolResult { .. },
                ) => {}

                // --- PER-SUBCALL USAGE ---
                rig::agent::MultiTurnStreamItem::CompletionCall(call) => {
                    last_total_tokens = call.usage.total_tokens;
                }

                // --- FINAL RESPONSE ---
                rig::agent::MultiTurnStreamItem::FinalResponse(fin) => {
                    text = fin.output().to_string();
                    usage = fin.usage();
                    messages = fin.messages().map(|h| h.to_vec());
                }

                // MultiTurnStreamItem is #[non_exhaustive] — required wildcard arm.
                // Future rig versions may add new variants; we ignore them here.
                _ => {}
            },
            Some(Err(e)) => {
                // Check whether rig cancelled the agent loop via the hook's Terminate action.
                match e {
                    rig::agent::StreamingError::Prompt(boxed) => match *boxed {
                        rig::completion::PromptError::PromptCancelled {
                            reason,
                            chat_history,
                        } => {
                            log::trace!(
                                "Stream PromptCancelled: history_len={}",
                                chat_history.len()
                            );
                            messages = Some(chat_history);
                            cancelled = true;
                            cancel_reason = Some(reason);
                            break;
                        }
                        other => {
                            return Err(rig::agent::StreamingError::Prompt(Box::new(other)));
                        }
                    },
                    other => return Err(other),
                }
            }
            None => {
                log::trace!("Stream completed normally");
                break;
            }
        }
    }

    Ok(StreamingTurnResult {
        text,
        usage,
        messages,
        cancelled,
        cancel_reason,
        tool_call_count,
        deltas_emitted,
        last_total_tokens,
    })
}
