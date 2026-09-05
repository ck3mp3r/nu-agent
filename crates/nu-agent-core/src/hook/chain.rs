//! `HookChain<P>` — composable hook implementation built from named concern structs.
//!
//! Each concern (cancellation, sub-turn cap, doom loop, circuit breaker, history
//! snapshot) lives in its own module and has its own unit tests. `HookChain` owns
//! them as named fields and delegates to them in an explicit, readable order inside
//! the `AgentHook` impl.
//!
//! ## Adding a new concern
//!
//! 1. Create `new_concern.rs` + `new_concern_test.rs` under `hook/`.
//! 2. Add a named field on `HookChain`.
//! 3. Call the concern's method inside whichever event method needs it.
//! 4. Nothing else changes.

use std::sync::{Arc, Mutex};

use rig::agent::{
    AgentHook, CompletionCallAction, CompletionCallEvent, HookContext, InvalidToolCallAction,
    InvalidToolCallContext, ModelHandle, ModelSelection, ModelSelectionAction, ObservationAction,
    RequestPatch, StreamResponseFinish, TextDelta, ToolCall, ToolCallAction, ToolResultAction,
    ToolResultEvent,
};
use rig::core::wasm_compat::WasmCompatSend;
use rig::message::Message;

use crate::bus::{
    Bus, CancelEvent, CancelRx, CompactionEvent, LlmEvent, ToolEvent, TryRecvError, WarningEvent,
};
use crate::config::defaults;
use crate::conversation::compaction::CompactionConfig;
use crate::conversation::compaction::compactor::{NuCompactor, SummaryArtifact};
use crate::conversation::turn::token_estimate::estimate_token_count;
use crate::session::SessionStore;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;

use super::agent_hook::HookState;
use super::circuit_breaker_guard::CircuitBreakerGuard;
use super::doom_loop::DoomLoopDetector;
use super::history_snapshot::HistorySnapshot;
use super::permission_resolver::{
    AsyncPermissionResolver, PermissionDecision, resolve_tool_source,
};
use super::subturn_cap::SubTurnCap;

/// Composable hook that delegates to named concern structs in explicit order.
///
/// See module-level docs for the extension pattern.
#[derive(Clone)]
pub struct HookChain<
    P: AsyncPermissionResolver,
    S: SessionStore + Clone + Send + Sync = crate::session::SessionStoreBackend,
> {
    cancel_rx: Arc<Mutex<CancelRx>>,
    subturn: SubTurnCap,
    doom: DoomLoopDetector,
    circuit: CircuitBreakerGuard,
    history: HistorySnapshot,
    permission: P,
    /// Shared signal bus — tool/LLM/warning events are published here.
    bus: Bus,
    closure_registry: Arc<ClosureRegistry>,
    mcp_registry: Arc<McpToolRegistry>,
    /// Shared runtime model handle. The single point of model identity: the agent
    /// is built from this handle and `on_model_select` routes every turn to its
    /// current value. `switch_model()` is the only writer. It is constructed
    /// eagerly at startup.
    shared_model: Arc<Mutex<ModelHandle>>,
    /// Memory backing the conversation, used to read markers and (optionally) reset
    /// the cache after compaction. Shared with the turn executor via `Arc`.
    memory: crate::conversation::state::memory::MemoryOf<S>,
    /// The session/conversation id this hook compacts.
    conversation_id: String,
    /// Hook-driven compaction machinery: compactor, policy, threshold.
    compaction: CompactionConfig<S>,
    /// Real token count from the last LLM completion, shared across turns. Used by
    /// `decide_compaction` to check the threshold against the real context size
    /// (plus the current prompt) instead of the chars/4 heuristic. `None` before
    /// the first completion and after a compaction reset.
    last_total_tokens: Arc<Mutex<Option<u64>>>,
}

impl<P: AsyncPermissionResolver, S: SessionStore + Clone + Send + Sync> HookChain<P, S> {
    pub fn new(
        bus: Bus,
        permission_resolver: P,
        closure_registry: Arc<ClosureRegistry>,
        mcp_registry: Arc<McpToolRegistry>,
        max_tool_calls_per_subturn: Option<usize>,
        hook_state: HookState<S>,
    ) -> Self {
        Self {
            cancel_rx: Arc::new(Mutex::new(bus.cancel().subscribe())),
            subturn: SubTurnCap::new(
                max_tool_calls_per_subturn.unwrap_or(defaults::MAX_TOOL_CALLS_PER_SUBTURN),
            ),
            doom: DoomLoopDetector {
                state: hook_state.doom_state,
            },
            circuit: CircuitBreakerGuard {
                breaker: hook_state.circuit_breaker,
            },
            history: HistorySnapshot::default(),
            permission: permission_resolver,
            bus,
            closure_registry,
            mcp_registry,
            shared_model: hook_state.shared_model,
            memory: hook_state.memory,
            conversation_id: hook_state.conversation_id,
            compaction: hook_state.compaction,
            last_total_tokens: hook_state.last_total_tokens,
        }
    }

    /// Return a clone of the `Arc` that holds the most recent history snapshot.
    ///
    /// Callers clone this Arc **before** passing the hook into the agent builder
    /// (which consumes `self`), then read it back after a `CompletionError`.
    pub fn last_known_history(&self) -> std::sync::Arc<std::sync::Mutex<Vec<Message>>> {
        self.history.arc()
    }

    /// Returns `true` if a cancellation has been requested via the bus cancel channel.
    ///
    /// Consumes any pending cancel event. A `Lagged` error also counts as cancelled
    /// because the cancel channel is capacity-bounded and an overflowed buffer means
    /// the turn must stop.
    fn is_cancelled(&self) -> bool {
        let mut rx = self.cancel_rx.lock().expect("cancel_rx mutex poisoned");
        matches!(
            rx.try_recv(),
            Ok(CancelEvent::Requested) | Err(TryRecvError::Lagged(_))
        )
    }
}

impl<P: AsyncPermissionResolver, S: SessionStore + Clone + Send + Sync> AgentHook
    for HookChain<P, S>
{
    fn on_model_select(
        &self,
        _ctx: &HookContext,
        _event: ModelSelection<'_>,
    ) -> ModelSelectionAction {
        let guard = self.shared_model.lock().expect("model mutex poisoned");
        ModelSelectionAction::select(guard.clone())
    }

    fn on_completion_call(
        &self,
        _ctx: &HookContext,
        event: CompletionCallEvent<'_>,
    ) -> impl std::future::Future<Output = CompletionCallAction> + WasmCompatSend {
        // 1. History snapshot — store history + prompt before the LLM call
        self.history.update(event.history, event.prompt);

        // 2. Reset sub-turn cap — new LLM request, fresh counter
        self.subturn.reset();

        // Trace-level payload logging for debugging LLM request contents.
        if log::log_enabled!(log::Level::Trace) {
            log::trace!(
                "on_completion_call: history_len={} prompt={}",
                event.history.len(),
                serde_json::to_string(event.prompt).unwrap_or_else(|_| "<serialize error>".into()),
            );
            for (i, msg) in event.history.iter().enumerate() {
                log::trace!(
                    "  history[{}]: {}",
                    i,
                    serde_json::to_string(msg).unwrap_or_else(|_| "<serialize error>".into()),
                );
            }
        } else if log::log_enabled!(log::Level::Debug) {
            log::trace!("on_completion_call: history_len={}", event.history.len());
        }

        let cancelled = self.is_cancelled();
        let bus = self.bus.clone();
        let memory = Arc::clone(&self.memory);
        let conversation_id = self.conversation_id.clone();
        let compaction = self.compaction.clone();
        let last_total_tokens = Arc::clone(&self.last_total_tokens);
        // Cloned history for the compaction decision; rig owns `event.history`.
        let history: Vec<Message> = event.history.to_vec();
        let prompt = event.prompt.clone();

        async move {
            if cancelled {
                return CompletionCallAction::stop("Cancelled by user");
            }
            let _ = bus.llm().send(LlmEvent::Started).await;

            // Compaction decision: patch the per-turn history when a marker
            // already summarizes the prefix, or when a new compaction is needed.
            if let Some(action) = decide_compaction(
                &history,
                &prompt,
                conversation_id.as_str(),
                memory.as_ref(),
                &compaction,
                &last_total_tokens,
                &bus,
            )
            .await
            {
                return action;
            }

            CompletionCallAction::continue_run()
        }
    }

    fn on_text_delta(
        &self,
        _ctx: &HookContext,
        event: TextDelta<'_>,
    ) -> impl std::future::Future<Output = ObservationAction> + WasmCompatSend {
        let cancelled = self.is_cancelled();
        let bus = self.bus.clone();
        let text = event.aggregated.to_string();

        async move {
            if cancelled {
                return ObservationAction::stop("Cancelled by user");
            }
            let _ = bus.llm().send(LlmEvent::AssistantMessage { text }).await;
            ObservationAction::continue_run()
        }
    }

    fn on_tool_call(
        &self,
        _ctx: &HookContext,
        event: ToolCall<'_>,
    ) -> impl std::future::Future<Output = ToolCallAction> + WasmCompatSend {
        let tool_name = event.tool_name;
        let args = event.args;
        let tool_call_id = event.tool_call_id;

        log::trace!("on_tool_call: tool={tool_name}");

        // Pre-compute all synchronous checks and capture values for the async block
        let cancelled = self.is_cancelled();
        let subturn_action = self.subturn.check_and_increment(tool_name);
        let circuit_action = self
            .circuit
            .check_server_enabled(tool_name, &self.mcp_registry);

        let source = resolve_tool_source(tool_name, &self.closure_registry, &self.mcp_registry)
            .as_str()
            .to_string();

        let permission = self.permission.clone();
        let bus = self.bus.clone();
        let tool_name_owned = tool_name.to_string();
        let args_owned = args.to_string();
        let source_owned = source.clone();
        let id_owned = tool_call_id.map(|s| s.to_string());

        async move {
            if cancelled {
                return ToolCallAction::stop("Cancelled by user");
            }
            if let Some(action) = subturn_action {
                return action;
            }
            let doom_action = self
                .doom
                .check_and_record(&tool_name_owned, &args_owned, &bus)
                .await;
            if let Some(action) = doom_action {
                return action;
            }
            if let Some(action) = circuit_action {
                return action;
            }

            let _ = bus
                .tool()
                .send(ToolEvent::Started {
                    name: tool_name_owned.clone(),
                    source: source_owned.clone(),
                    arguments: args_owned.clone(),
                })
                .await;

            let decision = permission
                .resolve(&tool_name_owned, &args_owned, id_owned, &bus)
                .await;
            match decision {
                PermissionDecision::Allow => ToolCallAction::run(),
                PermissionDecision::Deny { reason } => {
                    let _ = bus
                        .tool()
                        .send(ToolEvent::Completed {
                            name: tool_name_owned,
                            source: source_owned,
                            arguments: args_owned,
                            success: false,
                            result: String::new(),
                            display: None,
                            error_kind: None,
                            message: Some(reason.clone()),
                        })
                        .await;
                    ToolCallAction::skip(reason)
                }
            }
        }
    }

    fn on_tool_result(
        &self,
        _ctx: &HookContext,
        event: ToolResultEvent<'_>,
    ) -> impl std::future::Future<Output = ToolResultAction> + WasmCompatSend {
        let tool_name = event.tool_name;
        let args = event.args;
        let result = event.presentation;
        let result_text = result.render();

        // Structural success: the canonical result disposition decides. A
        // non-zero `nu` exit arrives as an Error disposition (producers are
        // honest), and Refused/Skipped never count as success — no result-text
        // sniffing.
        let success = event.raw_result.is_success();

        // Record the verdict for persistence: `CachedMemory::append` stamps it
        // onto the ToolResult with the matching call id when the turn is
        // written to the session store.
        if let Some(id) = event.tool_call_id {
            self.memory.record_tool_verdict(id, success);
        }

        log::trace!(
            "on_tool_result: tool={tool_name} success={success} result_len={}",
            result_text.len(),
        );
        if log::log_enabled!(log::Level::Trace) {
            let preview = if result_text.len() > 2000 {
                format!(
                    "{}...<truncated {} bytes>",
                    &result_text[..2000],
                    result_text.len()
                )
            } else {
                result_text.to_string()
            };
            log::trace!("  result_body: {preview}");
        }

        // 1. Parse result JSON and extract display
        let display = serde_json::from_str::<serde_json::Value>(&result_text)
            .ok()
            .and_then(|json| crate::tools::handler::build_direct_tool_display(tool_name, &json));

        // 2. Resolve source
        let source = resolve_tool_source(tool_name, &self.closure_registry, &self.mcp_registry)
            .as_str()
            .to_string();

        // 3. Classify error kind from raw result
        let error_kind = event
            .raw_result
            .error()
            .map(|e| e.kind().as_str().to_string());

        // 4. Emit ToolCompleted (success was computed structurally above)
        let tool_name_owned = tool_name.to_string();
        let args_owned = args.to_string();
        let result_text_owned = result_text.to_string();
        let source_owned = source;
        let display_owned = display;
        let error_kind_owned = error_kind;
        let bus = self.bus.clone();
        let circuit = self.circuit.clone();
        let mcp_registry = Arc::clone(&self.mcp_registry);

        async move {
            let _ = bus
                .tool()
                .send(ToolEvent::Completed {
                    name: tool_name_owned.clone(),
                    source: source_owned,
                    arguments: args_owned.clone(),
                    success,
                    result: result_text_owned.clone(),
                    display: display_owned,
                    error_kind: error_kind_owned,
                    message: None,
                })
                .await;

            // 5. Circuit breaker — track transport failures per server
            circuit
                .record_result(
                    &tool_name_owned,
                    event.raw_result,
                    success,
                    &mcp_registry,
                    &bus,
                )
                .await;

            ToolResultAction::keep()
        }
    }

    fn on_stream_response_finish(
        &self,
        _ctx: &HookContext,
        event: StreamResponseFinish<'_>,
    ) -> impl std::future::Future<Output = ObservationAction> + WasmCompatSend {
        let usage = event.usage;
        let completed = if usage.total_tokens > 0 {
            let mut response_chars = 0usize;
            let mut tool_calls = 0usize;
            for item in event.content.iter() {
                match item {
                    rig::message::AssistantContent::Text(text) => {
                        response_chars += text.text.chars().count();
                    }
                    rig::message::AssistantContent::ToolCall(_) => tool_calls += 1,
                    _ => {}
                }
            }
            // Store the real API token count for the compaction threshold check.
            // Mutex poison is a fatal internal inconsistency — panicking is correct.
            *self.last_total_tokens.lock().unwrap() = Some(usage.total_tokens);
            Some(LlmEvent::Completed {
                response_chars,
                tool_calls,
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                total_tokens: usage.total_tokens,
            })
        } else {
            None
        };
        let bus = self.bus.clone();
        async move {
            if let Some(event) = completed {
                let _ = bus.llm().send(event).await;
            }
            ObservationAction::continue_run()
        }
    }

    fn on_invalid_tool_call(
        &self,
        _ctx: &HookContext,
        event: &InvalidToolCallContext,
    ) -> impl std::future::Future<Output = Option<InvalidToolCallAction>> + WasmCompatSend {
        log::warn!("Invalid tool call: tool={}", event.tool_name);
        let feedback = format!(
            "Tool '{}' is not available. Available tools: [{}]",
            event.tool_name,
            event.available_tools.join(", ")
        );
        let bus = self.bus.clone();
        async move {
            let _ = bus
                .warning()
                .send(WarningEvent::Message {
                    message: feedback.clone(),
                })
                .await;
            Some(InvalidToolCallAction::retry(feedback))
        }
    }
}

// region: --- Compaction

/// Decide whether and how to patch the per-turn history for compaction.
///
/// Returns `Some(action)` when the history should be patched (or a stop is
/// needed); `None` when the model should see the full history unchanged
/// (`continue_run`).
///
/// This is the single place where compaction logic lives. The store and cache
/// always keep the full history; the patched history affects only what is sent
/// to the model this turn.
async fn decide_compaction<S>(
    history: &[Message],
    prompt: &Message,
    conversation_id: &str,
    memory: &crate::session::CachedMemory<S>,
    compaction: &CompactionConfig<S>,
    last_total_tokens: &Arc<Mutex<Option<u64>>>,
    bus: &Bus,
) -> Option<CompletionCallAction>
where
    S: SessionStore + Clone + Send + Sync,
{
    let threshold_tokens = compaction.threshold_tokens;

    // Empty history — nothing to compact or patch.
    if history.is_empty() {
        return None;
    }

    // Load the last marker for the session, if any, plus the messages that follow
    // it in the store (so we can patch from the marker without re-seeing the
    // summarized prefix). Build the per-turn CONTEXT from the marker summary (if
    // any) and the messages after it; when no marker exists, the context is the
    // full history.
    //
    // The threshold is checked against this context, NOT the full history. The
    // full history never shrinks (the store/cache keep everything for posterity),
    // so checking it would re-compact every turn after the first.
    let (last_marker, messages_after_marker) =
        load_marker_context(&compaction.compactor, memory, conversation_id, bus).await;

    let context: Vec<Message> = match &last_marker {
        Some(marker) => {
            let mut ctx = Vec::with_capacity(1 + messages_after_marker.len());
            ctx.push(Message::from(SummaryArtifact::from_marker_summary(
                marker.summary.clone(),
            )));
            ctx.extend(messages_after_marker.iter().cloned());
            ctx
        }
        None => history.to_vec(),
    };

    // Threshold check: use the real token count from the last completion when
    // available (it reflects the actual context the model processed last turn,
    // which already includes the prior prompt), plus a chars/4 estimate for the
    // current prompt. When no real count exists (first turn, or after a
    // compaction reset) fall back to the chars/4 estimate for the context too.
    // Mutex poison is a fatal internal inconsistency — panicking is correct.
    let base_tokens = last_total_tokens
        .lock()
        .unwrap()
        .and_then(|n| u64::try_into(n).ok())
        .unwrap_or_else(|| estimate_token_count(&context));
    let prompt_estimate = estimate_token_count(std::slice::from_ref(prompt));
    let total = base_tokens + prompt_estimate;
    let over_threshold = threshold_tokens.is_some_and(|limit| total >= limit);
    if !over_threshold {
        if last_marker.is_some() {
            return patch_from_marker(&messages_after_marker, &last_marker, bus).await;
        }
        return None;
    }

    // Compaction is needed (context is over the threshold). Fire a
    // `CompactionEvent::Requested` on the bus so the orchestrator schedules the
    // compaction on the worker. The current turn proceeds with the existing
    // marker patch (or the full history); the new marker is applied on the next
    // turn via `patch_from_marker`.
    let _ = bus
        .compaction()
        .send(CompactionEvent::Requested {
            source: "auto".to_string(),
        })
        .await;
    patch_from_marker(&messages_after_marker, &last_marker, bus).await
}

/// Run a compaction for `conversation_id`, returning the patched history
/// `[summary]` when compaction ran, or `None` when there was nothing to compact.
///
/// Called by the router when the orchestrator dispatches a `RunCompaction`
/// worker command. `source` is `"auto"` or `"slash"`. `compact()` emits all
/// `Started`/`SummaryChunk`/`Completed`/`Failed` events; this function only
/// returns `None` when there are no messages to summarize.
///
/// The context is `[previous_summary?, ...messages_since_last_marker]`. The
/// previous summary is passed to `compact()` as `carry_over`; the messages
/// since the last marker (WITHOUT the previous summary) are the messages to
/// summarize. The result is a single summary message — the summary IS the
/// context, no kept messages.
pub(crate) async fn run_compaction<S>(
    history: &[Message],
    conversation_id: &str,
    memory: &crate::session::CachedMemory<S>,
    compaction: &CompactionConfig<S>,
    source: &str,
    last_total_tokens: &Arc<Mutex<Option<u64>>>,
    bus: &Bus,
) -> Option<Vec<Message>>
where
    S: SessionStore + Clone + Send + Sync,
{
    // Load the last marker for the session, if any, plus the messages that follow
    // it in the store. When no marker exists, the messages to summarize are the
    // full history.
    let (last_marker, messages_after_marker) =
        load_marker_context(&compaction.compactor, memory, conversation_id, bus).await;

    let messages_to_summarize: Vec<Message> = match &last_marker {
        Some(_) => messages_after_marker,
        None => history.to_vec(),
    };

    // Nothing to compact — no messages since the last marker and no history.
    // Emit a `Completed` with an empty summary so the TUI gets feedback that
    // the `/compact` command ran even when there was nothing to compact.
    if messages_to_summarize.is_empty() {
        let _ = bus
            .compaction()
            .send(CompactionEvent::Completed {
                source: source.to_string(),
                summary_preview: String::new(),
                summary_body: String::new(),
            })
            .await;
        return None;
    }

    // Run a fresh compaction with the marker summary as carry-over (so the new
    // summary preserves prior context). `compact()` emits all lifecycle events.
    let carry_over = last_marker
        .as_ref()
        .map(|m| SummaryArtifact::from_marker_summary(m.summary.clone()));
    let artifact = match compaction
        .compactor
        .compact(
            conversation_id,
            &messages_to_summarize,
            carry_over.as_ref(),
            source,
        )
        .await
    {
        Ok(artifact) => artifact,
        Err(_) => {
            // `compact()` already emitted `Failed`; fall back to the full history
            // so the turn is not lost.
            return None;
        }
    };

    let summary_message = Message::from(artifact);
    // Compaction succeeded: the real token count from the previous turn no longer
    // reflects the (now summarized) context. Reset it so the next turn falls back
    // to the estimate (small — just the summary), then picks up the real count
    // after its first completion. Mutex poison is fatal — panicking is correct.
    *last_total_tokens.lock().unwrap() = None;
    Some(vec![summary_message])
}

/// Load the last `CompactionMarker` for `conversation_id` and the messages that
/// follow it in the store.
///
/// Store errors are surfaced as a `CompactionEvent::Failed` on `bus` and fall
/// back to `(None, Vec::new())` so the caller degrades gracefully but the
/// failure is never silently swallowed.
async fn load_marker_context<S>(
    compactor: &NuCompactor<S>,
    memory: &crate::session::CachedMemory<S>,
    conversation_id: &str,
    bus: &Bus,
) -> (Option<crate::session::CompactionMarker>, Vec<Message>)
where
    S: SessionStore + Clone + Send + Sync,
{
    let marker = match compactor.last_marker(conversation_id, "auto").await {
        Ok(marker) => marker,
        Err(e) => {
            let _ = bus
                .compaction()
                .send(CompactionEvent::Failed {
                    source: "auto".to_string(),
                    message: format!("Failed to load marker: {e}"),
                })
                .await;
            return (None, Vec::new());
        }
    };
    let messages_after_marker = match &marker {
        Some(_) => {
            let entries = match memory.load_all(conversation_id).await {
                Ok(entries) => entries,
                Err(e) => {
                    let _ = bus
                        .compaction()
                        .send(CompactionEvent::Failed {
                            source: "auto".to_string(),
                            message: format!("Failed to load session entries: {e}"),
                        })
                        .await;
                    return (None, Vec::new());
                }
            };
            // Find the marker by the last `StoreEntry::Marker` index, not by
            // summary-text equality (which could match an empty marker).
            let marker_idx = entries
                .iter()
                .rposition(|e| matches!(e, crate::session::StoreEntry::Marker(_)));
            match marker_idx {
                Some(idx) => entries[idx + 1..]
                    .iter()
                    .filter_map(|e| match e {
                        crate::session::StoreEntry::Message(m) => Some(m.clone()),
                        _ => None,
                    })
                    .collect(),
                None => Vec::new(),
            }
        }
        None => Vec::new(),
    };
    (marker, messages_after_marker)
}

/// If a marker exists, return a patch built from `[summary, ...messages_after_marker]`.
///
/// Returns `None` when no marker is present. An empty-summary marker is
/// surfaced as a `CompactionEvent::Failed` on `bus` instead of silently
/// degrading to the full history.
async fn patch_from_marker(
    messages_after_marker: &[Message],
    last_marker: &Option<crate::session::CompactionMarker>,
    bus: &Bus,
) -> Option<CompletionCallAction> {
    let Some(marker) = last_marker else {
        return None;
    };
    if marker.summary.is_empty() {
        let _ = bus
            .compaction()
            .send(CompactionEvent::Failed {
                source: "hook".to_string(),
                message: "Marker has empty summary, cannot patch".to_string(),
            })
            .await;
        return None;
    }
    let summary_message =
        Message::from(SummaryArtifact::from_marker_summary(marker.summary.clone()));
    let mut patched = Vec::with_capacity(1 + messages_after_marker.len());
    patched.push(summary_message);
    patched.extend_from_slice(messages_after_marker);
    Some(CompletionCallAction::patch(
        RequestPatch::new().history(patched),
    ))
}

// endregion: --- Compaction

#[cfg(test)]
#[path = "chain_test.rs"]
mod chain_test;
