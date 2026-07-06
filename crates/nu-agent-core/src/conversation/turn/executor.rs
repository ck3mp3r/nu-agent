//! Owns the logic for executing a single agent turn: building the rig agent,
//! dispatching through the hook, handling cancel paths, and persisting results.
//!
//! Extracted from `AgentConversationRuntime::execute_turn` to give it a single
//! responsibility. `AgentConversationRuntime` constructs a `TurnExecutor` and delegates.

use std::sync::{Arc, Mutex};

use nu_protocol::{LabeledError, Span, Value};
use rig::memory::ConversationMemory;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::conversation::managers::SessionManager;
use crate::conversation::providers::{CachedProviderClient, ModelVisitor};
use crate::conversation::turn::{
    TurnContext, TurnResult, error, execute_turn, execute_turn_with_channel,
};
use crate::hook::agent_hook::DoomLoopState;
use crate::hook::permission_resolver::AsyncPermissionResolver;
use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;
use crate::session::JournalConversationMemory;
use crate::session::repair::inject_missing_tool_results;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::tools::mcp::circuit_breaker::McpCircuitBreaker;
use crate::types::{Message, ToolDefinition};

/// Outcome of `TurnExecutor::execute` — either a completed turn whose results
/// have been persisted and whose UI events have been emitted, or an early-exit
/// value (cancelled / error) that the caller can return directly.
#[derive(Debug)]
pub enum TurnOutcome {
    /// The turn completed normally. The delegate should evaluate auto-compaction
    /// and then build the final `Value` using `build_response`.
    Completed,
    /// Early exit — the caller should return the contained `Value` directly.
    /// Used for cancellation paths where a minimal response is returned.
    EarlyReturn(Value),
}

/// Bundles the per-turn input parameters for [`TurnExecutor::execute`].
pub struct ExecuteInput {
    pub prompt: String,
    pub preamble: Option<String>,
    pub span: Span,
}

/// Data captured during a completed turn, used by `build_response` to construct
/// the final `Value`. Allows the response to be built after the executor's
/// borrows are released (so the delegate can run compaction in between).
pub struct TurnResponseData {
    pub text: String,
    pub usage: rig::completion::request::Usage,
    pub has_session: bool,
}

/// Groups the tool infrastructure fields always passed through to TurnContext.
pub struct ToolInfra {
    pub closure_registry: Arc<ClosureRegistry>,
    pub mcp_registry: Arc<McpToolRegistry>,
    pub tool_server_handle: rig::tool::server::ToolServerHandle,
    pub visible_tool_definitions: Vec<ToolDefinition>,
    pub circuit_breaker: Arc<Mutex<McpCircuitBreaker>>,
    pub doom_state: Arc<Mutex<DoomLoopState>>,
}

pub struct TurnExecutor<'a, S: SessionManager> {
    pub config: &'a Config,
    pub runtime: &'a tokio::runtime::Runtime,
    pub memory_state: &'a mut S,
    pub tool_infra: ToolInfra,
    /// Stored after a completed turn so the delegate can extract it for response formatting.
    response_data: Option<TurnResponseData>,
}

impl<'a, S: SessionManager> TurnExecutor<'a, S> {
    pub fn new(
        config: &'a Config,
        runtime: &'a tokio::runtime::Runtime,
        memory_state: &'a mut S,
        tool_infra: ToolInfra,
    ) -> Self {
        Self {
            config,
            runtime,
            memory_state,
            tool_infra,
            response_data: None,
        }
    }

    /// Extract the response data captured during `execute`. Returns `None` if
    /// `execute` was not called or did not complete normally.
    pub fn take_response_data(&mut self) -> Option<TurnResponseData> {
        self.response_data.take()
    }

    /// Execute the turn: dispatch through the provider visitor, handle cancellation
    /// paths, persist messages, and emit UI events. Returns `TurnOutcome::Completed`
    /// on success (caller should then evaluate compaction and call `build_response`),
    /// or `TurnOutcome::EarlyReturn(value)` for cancellation paths.
    ///
    /// The optional `ui_channel` is used in TUI mode: the caller creates `(ui_tx, ui_rx)`
    /// and passes the pair here so the drain loop and the `AgentHook` share the same
    /// channel. The hook passes its `ui_tx` to the permission resolver on each call.
    /// Pass `None` for TTY/policy mode (the channel is created internally).
    pub fn execute<U: ProgressUi, P: AsyncPermissionResolver>(
        &mut self,
        ui: &mut U,
        input: ExecuteInput,
        cached_client: &CachedProviderClient,
        permission_resolver: P,
        final_session_id: Option<&str>,
        mut ui_channel: Option<(
            mpsc::UnboundedSender<UiEvent>,
            mpsc::UnboundedReceiver<UiEvent>,
        )>,
    ) -> Result<TurnOutcome, LabeledError> {
        let prompt = input.prompt;
        let preamble = input.preamble;
        let span = input.span;
        let conversation_id = if let Some(session_id) = final_session_id {
            session_id.to_string()
        } else {
            // No session: use transient ID based on timestamp
            format!(
                "transient-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            )
        };

        let model_name = self.config.model.clone();

        // Save tool definitions for retry (std::mem::take consumes them on each attempt)
        let saved_tool_definitions = self.tool_infra.visible_tool_definitions.clone();

        let mut attempt = 0u8;
        let visitor_result = loop {
            // Restore tool definitions on retry attempts
            if attempt > 0 {
                self.tool_infra.visible_tool_definitions = saved_tool_definitions.clone();
            }

            // Reset doom loop state at the start of every turn attempt
            self.tool_infra
                .doom_state
                .lock()
                .expect("doom loop mutex poisoned")
                .reset();

            let result = cached_client.with_model(
                &model_name,
                TurnVisitor {
                    runtime: self.runtime,
                    memory: self.memory_state.memory(),
                    config: self.config,
                    permission_resolver: permission_resolver.clone(),
                    closure_registry: self.tool_infra.closure_registry.clone(),
                    mcp_registry: self.tool_infra.mcp_registry.clone(),
                    tool_server_handle: &self.tool_infra.tool_server_handle,
                    ui,
                    prompt: prompt.clone(),
                    conversation_id: conversation_id.clone(),
                    has_session: final_session_id.is_some(),
                    preamble: preamble.clone(),
                    visible_tool_definitions: std::mem::take(
                        &mut self.tool_infra.visible_tool_definitions,
                    ),
                    // Only pass the channel on the first attempt; retries create their own internally
                    ui_channel: if attempt == 0 {
                        ui_channel.take()
                    } else {
                        None
                    },
                    circuit_breaker: self.tool_infra.circuit_breaker.clone(),
                    doom_state: self.tool_infra.doom_state.clone(),
                },
            );

            match &result {
                Err((
                    crate::conversation::turn::TurnError::CompletionFailed { kind, msg },
                    ctx,
                )) => {
                    // Hard error path — check if retryable using the pre-classified kind
                    let has_partial_history = !ctx.last_known_history.is_empty();
                    log::debug!(
                        "Hard error: kind={kind:?} retryable={} history_len={}",
                        kind.is_retryable(),
                        ctx.last_known_history.len()
                    );
                    if kind.is_retryable()
                        && attempt < self.config.max_retries.unwrap_or(3)
                        && has_partial_history
                    {
                        attempt += 1;
                        let base_delay = self
                            .config
                            .retry_base_delay_ms
                            .unwrap_or(1000)
                            .saturating_mul(1u64 << attempt.min(5))
                            .min(30_000);
                        // Random jitter: ±20% (0.8–1.2× multiplier) per Goose strategy
                        let jitter_factor = 0.8 + (rand::random::<f64>() * 0.4);
                        let raw_delay = if *kind == CompletionErrorKind::RateLimit {
                            extract_retry_after_ms(msg).unwrap_or(base_delay)
                        } else {
                            base_delay
                        };
                        let delay_ms = (raw_delay as f64 * jitter_factor) as u64;
                        log::warn!(
                            "Retryable error ({kind:?}), attempt {attempt}/{}. Retrying in {delay_ms}ms.",
                            self.config.max_retries.unwrap_or(3)
                        );
                        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                        continue;
                    }
                    // Exhausted retries — wrap the error with attempt count
                    if attempt > 0 {
                        log::warn!("Retries exhausted after {attempt} attempts: {kind:?}");
                        let retry_msg = format!("Turn failed after {attempt} retries. {msg}");
                        break Err((
                            crate::conversation::turn::TurnError::CompletionFailed {
                                msg: retry_msg,
                                kind: kind.clone(),
                            },
                            error::TurnContext {
                                last_known_history: ctx.last_known_history.clone(),
                                pre_turn_message_count: ctx.pre_turn_message_count,
                            },
                        ));
                    }
                    break result;
                }
                _ => break result,
            }
        };

        let turn_result =
            match visitor_result {
                Ok(result) => result,
                Err((
                    crate::conversation::turn::TurnError::MaxTurnsExceeded {
                        ref msg,
                        ref messages,
                        ..
                    },
                    ref ctx,
                ))
                | Err((
                    crate::conversation::turn::TurnError::UnknownTool {
                        ref msg,
                        ref messages,
                        ..
                    },
                    ref ctx,
                )) => {
                    // Path A (non-cancelled): MaxTurnsError / UnknownToolCall carry full chat_history.
                    // Persist only the delta (new messages from this turn) so the session remembers
                    // the failed turn without re-appending messages already in persistent storage.
                    if let Some(session_id) = final_session_id {
                        // messages for MaxTurnsError/UnknownToolCall is rig's full accumulated
                        // chat_history (from AgentRun::full_history()), which DOES include the
                        // current-turn user prompt and any tool call exchanges from this turn.
                        // This is unlike last_known_history (from on_completion_call's `history`
                        // parameter), which contains only prior messages. skip(pre_turn_message_count)
                        // correctly yields [user_prompt, assistant_tool_calls...] — the new messages.
                        let delta: Vec<Message> = messages
                            .iter()
                            .skip(ctx.pre_turn_message_count)
                            .cloned()
                            .collect();
                        log::debug!(
                            "Path A non-cancelled: delta_count={} pre_turn={}",
                            delta.len(),
                            ctx.pre_turn_message_count
                        );
                        if !delta.is_empty() {
                            let patched = inject_missing_tool_results(delta);
                            if let Err(mem_err) = self.runtime.block_on(
                                self.memory_state.memory_mut().append(session_id, patched),
                            ) {
                                log::warn!(
                                    "Failed to update context for failed turn (path A history): {}",
                                    mem_err
                                );
                            }
                        }
                    }
                    let msg_preview = &msg[..msg.len().min(200)];
                    log::error!("Turn error (path A non-cancelled): {msg_preview}");
                    let user_msg = msg.clone();
                    return Err(LabeledError::new(user_msg.clone()).with_label(user_msg, span));
                }
                Err((
                    crate::conversation::turn::TurnError::Cancelled { ref messages, .. },
                    ref ctx,
                )) => {
                    // Path A: rig hook cancelled — persist chat_history delta if available.
                    // messages for PromptCancelled is rig's full_history() — it DOES include
                    // the current-turn user prompt and any tool exchanges. skip(pre_turn_message_count)
                    // yields only the new messages from this turn, preventing double-appending the
                    // prior session history when a cancelled turn follows prior session messages.
                    if let Some(session_id) = final_session_id {
                        let delta: Vec<Message> = messages
                            .iter()
                            .skip(ctx.pre_turn_message_count)
                            .cloned()
                            .collect();
                        log::debug!(
                            "Path A cancelled: delta_count={} pre_turn={}",
                            delta.len(),
                            ctx.pre_turn_message_count
                        );
                        if !delta.is_empty() {
                            let patched = inject_missing_tool_results(delta);
                            let patched = close_open_tool_result_block(patched, "[cancelled]");
                            if let Err(mem_err) = self.runtime.block_on(
                                self.memory_state.memory_mut().append(session_id, patched),
                            ) {
                                log::warn!(
                                    "Failed to update context for cancelled turn (path A): {}",
                                    mem_err
                                );
                            }
                        }
                    }
                    // Return a minimal cancelled response (not an error)
                    let llm_response = crate::llm::LlmResponse {
                        text: String::new(),
                        usage: crate::llm::LlmUsage {
                            input_tokens: 0,
                            output_tokens: 0,
                            total_tokens: 0,
                            cached_input_tokens: 0,
                            cache_creation_input_tokens: 0,
                        },
                        tool_calls: Vec::new(),
                        tool_call_metadata: Vec::new(),
                    };
                    return Ok(TurnOutcome::EarlyReturn(crate::llm::format_response(
                        &llm_response,
                        self.config,
                        final_session_id,
                        span,
                    )));
                }
                Err((e, ctx)) => {
                    // Hard error: CompletionFailed or ToolExecutionFailed.
                    // If on_completion_call fired at least once, we have a partial history snapshot
                    // from the hook Arc. Persist it so completed sub-turns are not lost.
                    // If no on_completion_call fired (failure before first HTTP request), fall back
                    // to a synthetic user+assistant placeholder pair to maintain alternating structure.

                    // Extract fields needed for history persistence and error message.
                    let (msg, kind_opt) = match &e {
                        crate::conversation::turn::TurnError::CompletionFailed { msg, kind } => {
                            (msg, Some(kind))
                        }
                        crate::conversation::turn::TurnError::ToolExecutionFailed { msg } => {
                            (msg, None)
                        }
                        // Other variants (Cancelled, MaxTurnsExceeded, UnknownTool) are already
                        // handled above; this catch-all is for exhaustiveness.
                        _ => {
                            return Err(
                                LabeledError::new(e.to_string()).with_label(e.to_string(), span)
                            );
                        }
                    };

                    // For retry-exhausted errors, the message already contains the retry count.
                    // Otherwise, use the pre-classified kind to build the user-facing message.
                    let user_msg = if msg.starts_with("Turn failed after") {
                        msg.clone()
                    } else if let Some(kind) = kind_opt {
                        kind_to_user_msg(kind, msg)
                    } else {
                        // ToolExecutionFailed — no kind, use raw message
                        format!("Turn failed: {msg}")
                    };

                    log::error!(
                        "Turn failed with unrecoverable error: session={:?} error={}",
                        final_session_id,
                        &msg[..msg.len().min(200)]
                    );
                    if let Some(session_id) = final_session_id {
                        let delta: Vec<Message> = ctx
                            .last_known_history
                            .iter()
                            .skip(ctx.pre_turn_message_count)
                            .cloned()
                            .collect();
                        log::debug!(
                            "Hard error: delta_count={} pre_turn={}",
                            delta.len(),
                            ctx.pre_turn_message_count
                        );
                        if !delta.is_empty() {
                            let patched = inject_missing_tool_results(delta);
                            let patched = close_open_tool_result_block(patched, msg);
                            if let Err(mem_err) = self.runtime.block_on(
                                self.memory_state.memory_mut().append(session_id, patched),
                            ) {
                                log::warn!(
                                    "Failed to persist recovered history on hard error: {}",
                                    mem_err
                                );
                            }
                        } else {
                            // delta is empty: hook never fired OR error before any new messages
                            // were added. Synthesise a placeholder to maintain the alternating
                            // user/assistant structure.
                            log::debug!(
                                "Hard error: delta empty, synthesizing fallback placeholder"
                            );
                            let fallback = vec![
                                Message::user(prompt.clone()),
                                Message::assistant(format!("[Turn failed: {msg}]")),
                            ];
                            if let Err(mem_err) = self.runtime.block_on(
                                self.memory_state.memory_mut().append(session_id, fallback),
                            ) {
                                log::warn!(
                                    "Failed to persist error placeholder on hard error: {}",
                                    mem_err
                                );
                            }
                        }
                    }
                    return Err(LabeledError::new(user_msg.clone()).with_label(user_msg, span));
                }
            };

        // Path C: PromptCancelled was caught inside build_agent_and_stream and returned
        // as Ok(cancelled=true). Treat identically to Path A.
        //
        // Path B (defensive fallback) is folded into Path C: Ok(cancelled=true, messages=None).
        // Under rig v0.39+ semantics this branch is not expected to trigger — PromptCancelled
        // always carries chat_history. Kept as a safety net for future rig version changes.
        if turn_result.cancelled {
            if let Some(session_id) = final_session_id {
                if let Some(ref messages) = turn_result.messages {
                    // Normal path: rig provided chat_history via PromptCancelled.
                    // PromptCancelled.chat_history is rig's full_history() — it DOES include
                    // the current-turn user prompt and any tool exchanges. skip(pre_turn_message_count)
                    // yields only the new messages from this turn, preventing double-appending the
                    // prior session history when a cancelled turn follows prior session messages.
                    // JournalConversationMemory.append() writes both JSONL and in-memory
                    // cache in one call — no separate conversation_store.append() needed.
                    let delta: Vec<Message> = messages
                        .iter()
                        .skip(turn_result.pre_turn_message_count)
                        .cloned()
                        .collect();
                    log::debug!(
                        "Path C cancelled: delta_count={} pre_turn={}",
                        delta.len(),
                        turn_result.pre_turn_message_count
                    );
                    if !delta.is_empty() {
                        let patched = inject_missing_tool_results(delta);
                        let patched = close_open_tool_result_block(patched, "[cancelled]");
                        if let Err(e) = self
                            .runtime
                            .block_on(self.memory_state.memory_mut().append(session_id, patched))
                        {
                            log::warn!(
                                "Failed to update context for cancelled turn (path C): {}",
                                e
                            );
                        }
                    }
                } else {
                    // Path B: tokio::select cancelled before rig yielded PromptCancelled.
                    // Use last_known_history from the hook's snapshot of completed work
                    // (including tool calls and their results) instead of synthesizing a
                    // minimal placeholder that would lose all completed work from this turn.
                    let lkh = &turn_result.last_known_history;
                    if !lkh.is_empty() {
                        let delta: Vec<Message> = lkh
                            .iter()
                            .skip(turn_result.pre_turn_message_count)
                            .cloned()
                            .collect();
                        log::debug!(
                            "Path B fallback: last_known_history delta_count={}",
                            delta.len()
                        );
                        if !delta.is_empty() {
                            let patched = inject_missing_tool_results(delta);
                            let patched = close_open_tool_result_block(patched, "[cancelled]");
                            if let Err(e) = self.runtime.block_on(
                                self.memory_state.memory_mut().append(session_id, patched),
                            ) {
                                log::warn!(
                                    "Failed to persist cancel path B with last_known_history: {}",
                                    e
                                );
                            }
                        }
                    } else {
                        // True fallback: hook never fired, synthesize minimal placeholder
                        log::debug!("Path B true fallback: synthesizing minimal placeholder");
                        let mut cancelled_messages = vec![Message::user(prompt.clone())];
                        if !turn_result.text.is_empty() {
                            cancelled_messages.push(Message::assistant(turn_result.text.clone()));
                        }
                        if let Err(e) = self.runtime.block_on(
                            self.memory_state
                                .memory_mut()
                                .append(session_id, cancelled_messages),
                        ) {
                            log::warn!("Failed to persist cancel path B fallback: {}", e);
                        }
                    }
                }
            }
            ui.emit(&UiEvent::Completed {
                tool_calls: turn_result.tool_call_count,
            });
            ui.flush();
            return Ok(TurnOutcome::EarlyReturn(crate::llm::format_response(
                &crate::llm::LlmResponse {
                    text: String::new(),
                    usage: crate::llm::LlmUsage {
                        input_tokens: 0,
                        output_tokens: 0,
                        total_tokens: 0,
                        cached_input_tokens: 0,
                        cache_creation_input_tokens: 0,
                    },
                    tool_calls: Vec::new(),
                    tool_call_metadata: Vec::new(),
                },
                self.config,
                final_session_id,
                span,
            )));
        }

        // Completed turn: update last_total_tokens for compaction tracking.
        // The actual JSONL write was already done by rig calling memory.append() at turn end.
        if final_session_id.is_some() {
            *self.memory_state.last_total_tokens_mut() = Some(turn_result.last_total_tokens);
            log::debug!(
                "Turn completed: session={:?} last_total_tokens={}",
                final_session_id,
                turn_result.last_total_tokens
            );
        }

        // Successful turn: reset doom loop state so accumulated signatures
        // from prior turns don't carry over into the next healthy turn.
        self.tool_infra.doom_state.lock().unwrap().reset();

        // Emit UI events
        if !turn_result.deltas_emitted {
            ui.emit(&UiEvent::AssistantMessage {
                text: turn_result.text.clone(),
            });
        }
        ui.emit(&UiEvent::Completed {
            tool_calls: turn_result.tool_call_count,
        });
        ui.flush();

        // Store turn data for response building
        self.response_data = Some(TurnResponseData {
            text: turn_result.text,
            usage: turn_result.usage,
            has_session: final_session_id.is_some(),
        });

        Ok(TurnOutcome::Completed)
    }
}

// ---------------------------------------------------------------------------
// TurnVisitor — module-level so it can be generic over P: AsyncPermissionResolver
// ---------------------------------------------------------------------------

struct TurnVisitor<'a, 'b, U, P> {
    runtime: &'a tokio::runtime::Runtime,
    memory: &'b JournalConversationMemory,
    config: &'a Config,
    permission_resolver: P,
    closure_registry: Arc<ClosureRegistry>,
    mcp_registry: Arc<McpToolRegistry>,
    tool_server_handle: &'a rig::tool::server::ToolServerHandle,
    ui: &'a mut U,
    prompt: String,
    conversation_id: String,
    /// Whether this turn belongs to a persistent session.
    ///
    /// Threaded into `TurnConversation` so `build_agent_and_stream` can skip
    /// `.memory()` for transient invocations and avoid writing orphan JSONL files.
    has_session: bool,
    preamble: Option<String>,
    visible_tool_definitions: Vec<ToolDefinition>,
    /// Pre-built channel for interactive (TUI) mode so the resolver's `ui_tx`
    /// and the drain loop's `ui_rx` share the same tokio unbounded channel.
    /// `None` for TTY/policy mode (channel created internally by `execute_turn`).
    ui_channel: Option<(
        mpsc::UnboundedSender<UiEvent>,
        mpsc::UnboundedReceiver<UiEvent>,
    )>,
    circuit_breaker: Arc<Mutex<McpCircuitBreaker>>,
    doom_state: Arc<Mutex<DoomLoopState>>,
}

impl<U: ProgressUi, P: AsyncPermissionResolver> ModelVisitor for TurnVisitor<'_, '_, U, P> {
    type Output = Result<TurnResult, (crate::conversation::turn::TurnError, error::TurnContext)>;

    fn visit<M>(self, model: M) -> Self::Output
    where
        M: rig::completion::CompletionModel + Clone + 'static,
    {
        let turn_ctx = TurnContext::new(
            self.runtime.handle(),
            model,
            super::TurnConversation {
                memory: self.memory.clone(),
                conversation_id: self.conversation_id,
                has_session: self.has_session,
            },
            super::TurnInput {
                prompt: self.prompt,
                preamble: self.preamble.as_deref(),
                max_turns: self.config.max_tool_turns,
            },
            ToolInfra {
                closure_registry: self.closure_registry.clone(),
                mcp_registry: self.mcp_registry.clone(),
                tool_server_handle: self.tool_server_handle.clone(),
                visible_tool_definitions: self.visible_tool_definitions,
                circuit_breaker: self.circuit_breaker.clone(),
                doom_state: self.doom_state.clone(),
            },
            self.config,
        );
        if let Some((ui_tx, ui_rx)) = self.ui_channel {
            execute_turn_with_channel(turn_ctx, self.ui, self.permission_resolver, ui_tx, ui_rx)
        } else {
            execute_turn(turn_ctx, self.ui, self.permission_resolver)
        }
    }
}

/// If the last message in `messages` is a `User` message whose content
/// consists entirely of `ToolResult` items, appends a synthetic assistant
/// message to close the tool block. This prevents `user(ToolResult) →
/// user(Text)` on the next turn, which both Anthropic and OpenAI reject.
/// Returns the (possibly extended) message list.
pub fn close_open_tool_result_block(messages: Vec<Message>, error_msg: &str) -> Vec<Message> {
    let needs_closing = matches!(
        messages.last(),
        Some(Message::User { content }) if content.iter().all(|c| matches!(c, crate::types::UserContent::ToolResult(_)))
    );
    if needs_closing {
        log::debug!("close_open_tool_result_block: appending synthetic assistant");
        let mut msgs = messages;
        msgs.push(Message::assistant(format!("[Turn failed: {error_msg}]")));
        msgs
    } else {
        messages
    }
}

/// Describes the category of a completion failure.
///
/// Used by callers to decide whether to retry, surface a user-readable message,
/// or trigger session repair.  The enum makes these decisions explicit rather
/// than embedding policy in string matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionErrorKind {
    /// 429 — temporary rate limit; retryable.
    RateLimit,
    /// 529/503 — provider overloaded; retryable.
    Overloaded,
    /// 500/504 — provider server error; retryable.
    ServerError,
    /// Transport or stream-decode error; retryable.
    Network,
    /// 413 — single request too large; permanent.
    RequestTooLarge,
    /// 400 context_length_exceeded — conversation too long; permanent.
    ContextOverflow,
    /// 400 with tool_use/tool_result — malformed tool sequence; permanent.
    ToolStructure,
    /// 401/403 — authentication or permission failure; permanent.
    Auth,
    /// 402 — billing limit; permanent.
    Quota,
    /// Credits exhausted on provider account; permanent.
    CreditsExhausted,
    /// Content policy or safety refusal; permanent.
    Refusal,
    /// 404 — endpoint not found; permanent.
    EndpointNotFound,
    /// Unrecognised error; treat as non-retryable until classified.
    Unknown,
}

impl CompletionErrorKind {
    /// Returns `true` for transient errors that are safe to retry.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimit | Self::Overloaded | Self::ServerError | Self::Network
        )
    }
}

/// Returns a user-visible error message for a given `CompletionErrorKind`.
///
/// The message describes what went wrong in human-readable terms. Called by the
/// hard-error path in `TurnExecutor::execute` to produce the `LabeledError` returned
/// to the Nushell caller.
fn kind_to_user_msg(kind: &CompletionErrorKind, raw_msg: &str) -> String {
    match kind {
        CompletionErrorKind::ToolStructure => {
            "Turn failed: the API rejected this turn — a tool call was missing its result. The session has been repaired.".to_string()
        }
        CompletionErrorKind::ContextOverflow => {
            "Turn failed: conversation too long. Run 'agent session compact' to summarise."
                .to_string()
        }
        CompletionErrorKind::RequestTooLarge => {
            "Turn failed: tool results too large. Lower max_tool_result_bytes in config."
                .to_string()
        }
        CompletionErrorKind::Refusal => {
            "Turn failed: the provider refused this request (content policy or safety filter)."
                .to_string()
        }
        CompletionErrorKind::CreditsExhausted => {
            "Turn failed: account credits exhausted. Top up your provider account.".to_string()
        }
        CompletionErrorKind::Quota => {
            "Turn failed: billing limit reached. Check your provider account.".to_string()
        }
        CompletionErrorKind::RateLimit => "Turn failed: rate limit reached.".to_string(),
        CompletionErrorKind::Overloaded => "Turn failed: provider overloaded.".to_string(),
        CompletionErrorKind::ServerError => "Turn failed: provider server error.".to_string(),
        CompletionErrorKind::Network => "Turn failed: network error.".to_string(),
        CompletionErrorKind::EndpointNotFound => {
            "Turn failed: API endpoint not found. Check your provider configuration.".to_string()
        }
        CompletionErrorKind::Auth => {
            "Turn failed: authentication failed. Check your API key.".to_string()
        }
        CompletionErrorKind::Unknown => format!("Turn failed: {raw_msg}"),
    }
}

/// Build the final response `Value` from turn data. Called by the delegate after
/// auto-compaction has been evaluated.
pub fn build_response(
    response_data: Option<TurnResponseData>,
    config: &Config,
    session_id: Option<&str>,
    span: Span,
) -> Value {
    let data = response_data.unwrap_or(TurnResponseData {
        text: String::new(),
        usage: rig::completion::request::Usage {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
            tool_use_prompt_tokens: 0,
            reasoning_tokens: 0,
        },
        has_session: false,
    });

    let message_count = 0;

    let llm_response = crate::llm::LlmResponse {
        text: data.text,
        usage: crate::llm::LlmUsage {
            input_tokens: data.usage.input_tokens,
            output_tokens: data.usage.output_tokens,
            total_tokens: data.usage.total_tokens,
            cached_input_tokens: data.usage.cached_input_tokens,
            cache_creation_input_tokens: data.usage.cache_creation_input_tokens,
        },
        tool_calls: Vec::new(),         // TODO: track tool calls in TurnResult
        tool_call_metadata: Vec::new(), // TODO: track tool metadata in TurnResult
    };

    let response_value = crate::llm::format_response(&llm_response, config, session_id, span);

    if data.has_session
        && let Ok(record) = response_value.as_record()
    {
        let mut new_record = record.clone();
        if let Some(meta_value) = new_record.get("_meta")
            && let Ok(meta_record) = meta_value.as_record()
        {
            let mut new_meta = meta_record.clone();
            new_meta.insert(
                "message_count".to_string(),
                Value::int(message_count as i64, span),
            );

            new_record.insert("_meta".to_string(), Value::record(new_meta, span));
            return Value::record(new_record, span);
        }
    }

    response_value
}

/// Extract a "retry after N seconds" value from an error message string.
///
/// Matches common patterns from provider error responses:
/// - "retry after 5 seconds"
/// - "retry_after: 10"
/// - "Retry-After: 30"
///
/// Returns the value in milliseconds, or `None` if no recognisable pattern is found.
pub fn extract_retry_after_ms(msg: &str) -> Option<u64> {
    let msg_lower = msg.to_lowercase();
    // Pattern: "retry after N" or "retry-after: N" or "retry_after: N"
    let patterns = ["retry after ", "retry-after: ", "retry_after: "];
    for pattern in patterns {
        let Some(idx) = msg_lower.find(pattern) else {
            continue;
        };
        let after = &msg[idx + pattern.len()..];
        // Parse the first contiguous digits after the pattern
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            continue;
        }
        let Ok(seconds) = digits.parse::<u64>() else {
            continue;
        };
        return Some(seconds.saturating_mul(1000));
    }
    None
}

#[cfg(test)]
#[path = "test_utils.rs"]
mod test_utils;

#[cfg(test)]
#[path = "executor_test.rs"]
mod executor_test;

#[cfg(test)]
#[path = "journey_test.rs"]
mod journey_test;
