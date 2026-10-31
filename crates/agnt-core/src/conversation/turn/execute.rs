//! Conversation turn execution using agent hooks.
//!
//! This module provides `execute_turn` which handles a single conversation turn:
//! sending user input to the LLM, executing tool calls via hooks, and returning
//! the final response. Permission and lifecycle events flow to consumers through
//! the shared `Bus`; core never threads a `ProgressUi` through the turn.

use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;

use crate::bus::Bus;
use crate::config::defaults;
use crate::hook::agent_hook::HookState;
use crate::hook::chain::{HookChain, HookEnv};
use crate::hook::permission_resolver::AsyncPermissionResolver;
use crate::hook::turn_tools::TurnToolRecord;
use crate::protocol::events::{SystemEvent, SystemKind};
use crate::types::{Message, Text, ToolCall, ToolDefinition, UserContent};
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

/// How long the stream loop keeps draining after a bus cancel signal.
///
/// rig's stop path answers the in-flight tool batch into the run before it
/// cancels, so the drain is what keeps that work: the settled tool results
/// reach the `Cancelled` history instead of being dropped with the stream.
pub(super) const CANCEL_GRACE: Duration = Duration::from_secs(2);

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
    /// History snapshot from hook, captured before the last LLM call.
    /// Populated from `HookChain::last_known_history()` Arc after the turn completes.
    /// The retry loop reads it to decide whether a retry has partial history.
    pub last_known_history: Vec<Message>,
    /// Number of messages the session's rig log held when the turn started.
    /// Recovery anchors the turn's delta on content: it starts at the last
    /// user message whose text is the turn prompt or ends with the
    /// `"\n\n{prompt}"` steering suffix. The count's only role is the skip
    /// guard: a log whose length moved since the turn started is left alone.
    pub pre_turn_message_count: usize,
    /// Every tool call the turn dispatched and the result each call settled
    /// with, in dispatch order. Populated from `HookChain::turn_tools()` after
    /// the turn completes. A stop drops the in-flight batch from rig's history,
    /// so this is the only record of those calls.
    pub tool_records: Vec<TurnToolRecord>,
    /// The calls the model committed this turn, in commit order, as the
    /// stream's `ToolCall` projection carried them. A stop can leave a
    /// batch's siblings undispatched, so this is the only trace of the calls
    /// the turn never ran.
    pub streamed_tool_calls: Vec<ToolCall>,
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
pub(crate) async fn execute_turn<P>(
    ctx: TurnContext<'_>,
    permission_resolver: P,
) -> Result<TurnResult, (TurnError, Box<error::TurnContext>)>
where
    P: AsyncPermissionResolver,
{
    log::info!("execute_turn: starting turn");

    // The rig-memory store and log this turn keeps its conversation in. A
    // session-less turn persists nothing, so it gets no memory.
    let rig_memory = if ctx.conversation.has_session {
        match ctx
            .conversation
            .memory
            .rig_store(&ctx.conversation.conversation_id)
        {
            Ok(store) => Some((store, ctx.conversation.conversation_id.clone())),
            Err(error) => {
                log::warn!("Failed to open the session store: {error}");
                None
            }
        }
    } else {
        None
    };

    // The conversation the run starts from: the same store the run loads, so
    // the count is exact. Recovery anchors the turn's delta on the prompt, so
    // the count's only role is the skip guard: a log whose length moved since
    // the turn started is left alone.
    let pre_turn_messages: Vec<Message> = match &rig_memory {
        Some((store, log)) => match rig_memory::load(&**store, log) {
            Ok(conversation) => conversation
                .messages()
                .iter()
                .map(|message| (**message).clone())
                .collect(),
            Err(error) => {
                log::warn!("Failed to load the session's pre-turn messages: {error}");
                vec![]
            }
        },
        None => vec![],
    };
    let pre_turn_message_count = pre_turn_messages.len();

    // Build the hook using HookChain<P> — no HookDriver needed.
    // Cancellation is driven through the shared bus's cancel channel.
    let bus = ctx.tool_infra.bus.clone();
    // Read the turn's model once: the hook prices with it and the agent is
    // built from it, so a switch landing mid-turn cannot split the two.
    let dyn_model = ctx
        .conversation
        .dyn_model
        .lock()
        .expect("model mutex poisoned")
        .clone();

    let hook = HookChain::new(
        HookEnv {
            bus: bus.clone(),
            cwd: ctx.tool_infra.cwd.clone(),
        },
        permission_resolver,
        ctx.tool_infra.closure_registry.clone(),
        ctx.tool_infra.mcp_registry.clone(),
        ctx.tool_infra.render_registry.clone(),
        ctx.config.max_tool_calls_per_subturn,
        HookState {
            circuit_breaker: ctx.tool_infra.circuit_breaker.clone(),
            doom_state: ctx.tool_infra.doom_state.clone(),
            output_repetition: ctx.tool_infra.output_repetition.clone(),
            repetition_guard: ctx.tool_infra.repetition_guard,
            memory: Arc::clone(&ctx.conversation.memory),
            conversation_id: ctx.conversation.conversation_id.clone(),
            compaction: ctx.conversation.compaction.clone(),
            model: dyn_model.clone(),
            last_total_tokens: ctx.tool_infra.last_total_tokens.clone(),
            turn_usage: ctx.tool_infra.turn_usage.clone(),
            session_usage: ctx.tool_infra.session_usage.clone(),
        },
    );

    // Clone the Arc BEFORE hook moves into config so we can read history after a
    // ProviderError (when rig does not provide chat_history in TurnError::messages).
    let last_known_history_arc = hook.last_known_history();
    // Clone the tool-log Arc for the same reason: the hook moves into the agent
    // builder, and a stop drops the in-flight batch from rig's history, so the
    // records are the only trace of those calls after the turn.
    let turn_tools_arc = hook.turn_tools();

    // Build the prompt message
    let user_message = Message::User {
        content: vec![UserContent::Text(Text::new(ctx.input.prompt))],
    };

    // Clone preamble for the 'static future
    let preamble_owned = ctx.input.preamble.map(|s| s.to_string());

    // Build and execute agent with hook
    let config = AgentPromptConfig {
        hook,
        dyn_model,
        preamble: preamble_owned,
        prompt: user_message,
        rig_memory,
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
        model: format!("{}/{}", ctx.config.provider, ctx.config.model),
        pre_turn_message_count,
        cancel_grace: ctx.cancel_grace,
    };

    let prompt_future = Box::pin(build_agent_and_stream(config));

    // Publish a context-window warning on the warning bus channel. Warnings are
    // read from the bus directly by the TUI render loop and the TTY stderr
    // subscriber.
    if let Some(limit) = ctx.config.model_context_tokens {
        let estimated = token_estimate::estimate_token_count(&pre_turn_messages);
        let threshold = (limit as f32
            * ctx
                .config
                .context_warning_threshold
                .unwrap_or(defaults::CONTEXT_WARNING_THRESHOLD)) as usize;
        log::debug!("execute_turn: token_estimate={estimated} threshold={threshold} limit={limit}");
        if estimated >= threshold {
            let _ = bus
                .ui_event()
                .send(SystemEvent::SystemMessage {
                    kind: SystemKind::Warning,
                    text: format!(
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
    // hook entry (on_completion_call, on_text_delta, or on_dispatch), causing rig to
    // yield Cancelled { chat_history }. If the HTTP request hangs before any hook
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
        let ctx = Box::new(error::TurnContext {
            last_known_history: last_known_history_arc
                .lock()
                .expect("mutex poisoned")
                .clone(),
            pre_turn_message_count,
            tool_records: turn_tools_arc.records(),
        });
        (err, ctx)
    })?;

    let response = join_result.map_err(|e| {
        let err = TurnError::from(*e);
        let ctx = Box::new(error::TurnContext {
            last_known_history: last_known_history_arc
                .lock()
                .expect("mutex poisoned")
                .clone(),
            pre_turn_message_count,
            tool_records: turn_tools_arc.records(),
        });
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
        last_known_history: last_known_history_arc.lock().unwrap().clone(),
        pre_turn_message_count: response.pre_turn_message_count,
        tool_records: turn_tools_arc.records(),
        streamed_tool_calls: response.streamed_tool_calls,
    })
}

/// Configuration for building and prompting an agent.
struct AgentPromptConfig<P: AsyncPermissionResolver> {
    hook: HookChain<P>,
    /// Erased completion model the agent is built from. Cloned out of the
    /// shared `Arc<Mutex<…>>` so a concurrent `switch_model()` only affects the
    /// next turn.
    dyn_model: rig::DynModel<rig::operation::Completion>,
    preamble: Option<String>,
    prompt: Message,
    /// The rig-memory store and log this turn keeps its conversation in. rig
    /// loads the conversation before the run and saves it once the run
    /// succeeds; `None` for a session-less turn, which persists nothing.
    rig_memory: Option<(Arc<dyn rig_memory::Store>, String)>,
    tool_server_handle: rig::tool::server::ToolServerHandle,
    visible_tool_definitions: Vec<ToolDefinition>,
    max_turns: Option<u32>,
    bus: Bus,
    additional_params: Option<serde_json::Value>,
    temperature: Option<f64>,
    max_tokens: Option<u64>,
    /// The model this turn runs, `vendor/model`, saved as the session's
    /// `rig::model` state record once the run persists.
    model: String,
    /// Number of messages the session's rig log held when the turn started.
    pre_turn_message_count: usize,
    /// How long the stream loop keeps draining after a bus cancel signal.
    /// Production passes [`CANCEL_GRACE`]; tests pass a short bound.
    cancel_grace: Duration,
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
    /// The calls the model committed this turn, in commit order, as the
    /// stream's `ToolCall` projection carried them.
    streamed_tool_calls: Vec<ToolCall>,
    /// Whether any text deltas were emitted (i.e. streaming was active)
    deltas_emitted: bool,
    /// Total tokens from the most recent CompletionCall event
    last_total_tokens: u64,
    /// Number of messages the session's rig log held when the turn started.
    pre_turn_message_count: usize,
}

/// Build an agent with a hook and execute a multi-turn streaming prompt loop.
///
/// The error is boxed: rig 0.44's `PromptError` is large enough to trip
/// `clippy::result_large_err`.
async fn build_agent_and_stream<P>(
    config: AgentPromptConfig<P>,
) -> Result<StreamingTurnResult, Box<rig::completion::PromptError>>
where
    P: AsyncPermissionResolver,
{
    let AgentPromptConfig {
        hook,
        dyn_model,
        preamble,
        prompt,
        rig_memory,
        tool_server_handle,
        visible_tool_definitions,
        max_turns,
        bus,
        additional_params,
        temperature,
        max_tokens,
        model,
        pre_turn_message_count,
        cancel_grace,
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

    // Build the agent from the turn's model. The executor clones the current
    // model out of the shared `Arc<Mutex<…>>` before each turn, so a
    // `switch_model()` between turns takes effect on the next turn.

    // The agent keeps its conversation in the rig-memory store: rig loads it
    // before the run and saves it once the run succeeds. A session-less turn
    // gets no store, so rig neither loads nor saves a conversation.
    let mut builder = rig::agent::AgentBuilder::new(dyn_model)
        .add_hook(hook)
        .dynamic_tools(proxy_tools);
    // Keep the store handle for the host-written state records below: rig saves
    // the conversation at run end, and the state records and listing are ours.
    let state_store = rig_memory.as_ref().map(|(store, _)| Arc::clone(store));
    if let Some((store, log)) = rig_memory {
        builder = builder.memory(store).conversation(log);
    }
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
        .prompt(prompt)
        .max_turns(effective_max_turns as usize)
        .max_invalid_tool_call_retries(3)
        .stream();

    let result = drive_stream(stream, bus, cancel_grace, pre_turn_message_count).await?;

    // rig saved the conversation at run end. Write the host-owned state records
    // and refresh the listing, so `SavedAgent::model()`/`title()` and the session
    // list see this turn. A cancelled turn saves nothing, so it writes nothing.
    if !result.cancelled
        && let Some(store) = &state_store
    {
        let title = result
            .messages
            .as_deref()
            .and_then(crate::session::rig_state::title_of_messages);
        if let Err(error) =
            crate::session::rig_state::write_state_records(&**store, &model, title.as_deref())
        {
            log::warn!("Failed to write session state records: {error}");
        }
    }

    Ok(result)
}

/// Drive the agent's stream to its terminal state.
///
/// `cancel_grace` bounds how long the loop keeps draining after the bus cancel
/// signal: rig answers the in-flight tool batch into the run before it cancels,
/// so the drain is what keeps the settled tool results in the `Cancelled`
/// history. A stream that ends on its own inside the window ran to completion,
/// so the turn reports `cancelled = false`.
async fn drive_stream<S>(
    stream: S,
    bus: Bus,
    cancel_grace: Duration,
    pre_turn_message_count: usize,
) -> Result<StreamingTurnResult, Box<rig::completion::PromptError>>
where
    S: futures::Stream<
            Item = Result<rig::agent::MultiTurnStreamItem, rig::completion::PromptError>,
        >,
{
    tokio::pin!(stream);

    let mut text = String::new();
    let mut usage = rig::completion::request::Usage::default();
    let mut messages: Option<Vec<Message>> = None;
    let mut tool_call_count: usize = 0;
    let mut streamed_tool_calls: Vec<ToolCall> = Vec::new();
    let mut last_total_tokens: u64 = 0;
    let mut cancelled = false;
    let mut cancel_reason: Option<String> = None;
    let mut deltas_emitted = false;
    let mut cancel_rx = bus.cancel().subscribe();
    // Set when the bus cancel signal fires. From then on the loop drains the
    // stream until rig yields its canonical `Cancelled` history or this
    // deadline passes: rig answers the in-flight tool batch into the run
    // before it cancels, so the drain is what keeps the settled results.
    let mut grace_deadline: Option<tokio::time::Instant> = None;

    loop {
        let item = match grace_deadline {
            Some(deadline) => {
                tokio::select! {
                    biased;
                    item = stream.next() => item,
                    _ = tokio::time::sleep_until(deadline) => {
                        log::trace!("Cancel grace expired; dropping the stream");
                        break;
                    }
                }
            }
            None => {
                tokio::select! {
                    biased;
                    item = stream.next() => item,
                    Ok(_) = cancel_rx.recv() => {
                        log::trace!("Stream cancelled via bus cancel channel");
                        cancelled = true;
                        grace_deadline = Some(tokio::time::Instant::now() + cancel_grace);
                        continue;
                    }
                }
            }
        };
        match item {
            Some(Ok(event)) => match event {
                // --- STREAMED ASSISTANT CONTENT ---
                rig::agent::MultiTurnStreamItem::StreamAssistantItem(content) => {
                    match content {
                        // TEXT DELTA
                        rig::streaming::Item::Event(rig::streaming::StreamEvent::Text {
                            text: delta,
                            ..
                        }) => {
                            text.push_str(&delta);
                            deltas_emitted = true;
                        }
                        // TOOL CALL ARGUMENTS (complete, post-assembly)
                        // Hook's on_dispatch has already resolved. The hook already
                        // emitted ToolStarted + permission events.
                        rig::streaming::Item::Event(rig::streaming::StreamEvent::Arguments {
                            ..
                        }) => {
                            tool_call_count += 1;
                        }
                        // PART STARTED — no v0.42 equivalent, nothing to do
                        rig::streaming::Item::Event(rig::streaming::StreamEvent::Start {
                            ..
                        }) => {
                            log::trace!("Stream part started");
                        }
                        // REASONING fragment — ignore for now
                        rig::streaming::Item::Event(rig::streaming::StreamEvent::Reasoning {
                            ..
                        }) => {}
                        // PART ENDED — finalized content, not needed here
                        rig::streaming::Item::Event(rig::streaming::StreamEvent::End {
                            ..
                        }) => {}
                        // Unknown provider-specific content — ignore
                        rig::streaming::Item::Unknown(_) => {}
                    }
                }

                // --- PER-SUBCALL USAGE ---
                rig::agent::MultiTurnStreamItem::CompletionCall(call) => {
                    last_total_tokens = call.usage.total_tokens.unwrap_or(0);
                }

                // --- FINAL RESPONSE ---
                rig::agent::MultiTurnStreamItem::FinalResponse(fin) => {
                    text = fin.output();
                    usage = fin.usage();
                    messages = Some(fin.messages().to_vec());
                }

                // --- MODEL TURN RETRIED ---
                // rig's contract: "Text and reasoning deltas emitted for this
                // turn were provisional. A consumer should discard or visually
                // reset output associated with `turn`." Reset the accumulated
                // text and the delta flag so the retry's output overwrites
                // cleanly, and signal the TUI to discard the provisional
                // streaming block (empty reason = silent discard).
                rig::agent::MultiTurnStreamItem::ModelTurnRetried { .. } => {
                    deltas_emitted = false;
                    text.clear();
                    let _ = bus
                        .ui_event()
                        .send(SystemEvent::InferenceStopped {
                            reason: String::new(),
                        })
                        .await;
                }

                // --- COMMITTED TOOL CALL ---
                // The committed-call projection: history holds this call, so
                // the model emitted it this turn. Tracked for the cancel
                // path's reconciliation — a stop can leave a batch's
                // siblings undispatched, and only the stream names them.
                rig::agent::MultiTurnStreamItem::ToolCall { tool_call } => {
                    streamed_tool_calls.push(tool_call);
                }

                // MultiTurnStreamItem is #[non_exhaustive] — required wildcard arm.
                // Future rig versions may add new variants; we ignore them here.
                _ => {}
            },
            Some(Err(e)) => {
                // Check whether rig cancelled the agent loop via the hook's Terminate action.
                match e {
                    rig::completion::PromptError::Cancelled {
                        reason,
                        chat_history,
                    } => {
                        log::trace!("Stream Cancelled: history_len={}", chat_history.len());
                        messages = Some(chat_history);
                        cancelled = true;
                        cancel_reason = Some(reason);
                        break;
                    }
                    other => return Err(Box::new(other)),
                }
            }
            None => {
                log::trace!("Stream completed normally");
                // A stream that ends on its own during the grace window ran to
                // completion: rig saved the turn, so the cancel arrived too
                // late to matter and the turn completes normally.
                if grace_deadline.is_some() && messages.is_some() {
                    cancelled = false;
                }
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
        streamed_tool_calls,
        deltas_emitted,
        last_total_tokens,
        pre_turn_message_count,
    })
}

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/conversation/turn/execute.rs"]
mod execute_test;

// endregion: --- Tests
