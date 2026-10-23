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
    AgentHook, CompletionCallAction, CompletionCallEvent, DispatchAction, DispatchEvent,
    HookContext, InvalidToolCallAction, InvalidToolCallContext, ModelSelection,
    ModelSelectionAction, ModelTurnAction, ModelTurnFinished, ObservationAction, OutcomeAction,
    OutcomeEvent, TextDelta,
};
use rig::message::Message;
use rig::wasm_compat::WasmCompatSend;
use rig_memory::CompactReason;

use crate::bus::{Bus, CancelEvent, CancelRx, CompactionEvent, TryRecvError};
use crate::config::defaults;
use crate::conversation::compaction::CompactionConfig;
use crate::conversation::turn::token_estimate::estimate_token_count;
use crate::llm::{PricedOutcome, PricingSkip, computed_cost};
use crate::protocol::event::UiEvent;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::tools::handler::builtin_tool::ToolRenderRegistry;

use super::agent_hook::HookState;
use super::circuit_breaker_guard::CircuitBreakerGuard;
use super::doom_loop::DoomLoopDetector;
use super::history_snapshot::HistorySnapshot;
use super::output_repetition::{
    OUTPUT_REPETITION_STOP_PREFIX, OutputRepetitionDetector, RepetitionDetection, RepetitionState,
};
use super::permission_resolver::{
    AsyncPermissionResolver, PermissionDecision, resolve_tool_source,
};
use super::subturn_cap::SubTurnCap;
use super::unpriced_warnings::UnpricedWarnings;

/// Composable hook that delegates to named concern structs in explicit order.
///
/// See module-level docs for the extension pattern.
#[derive(Clone)]
pub struct HookChain<P: AsyncPermissionResolver> {
    cancel_rx: Arc<Mutex<CancelRx>>,
    subturn: SubTurnCap,
    doom: DoomLoopDetector,
    output_repetition: Arc<Mutex<RepetitionState>>,
    /// Enable/disable the repetition guard (doom-loop + output-repetition
    /// detection). When `false`, the detector call sites are skipped.
    repetition_guard: bool,
    circuit: CircuitBreakerGuard,
    history: HistorySnapshot,
    permission: P,
    /// Shared signal bus — tool/LLM/warning events are published here.
    bus: Bus,
    closure_registry: Arc<ClosureRegistry>,
    mcp_registry: Arc<McpToolRegistry>,
    /// Per-tool call-line render functions, populated at builtin registration
    /// time. Consulted in `on_dispatch` to build the transcript call line.
    render_registry: ToolRenderRegistry,
    /// Memory backing the conversation, used to read markers and (optionally) reset
    /// the cache after compaction. Shared with the turn executor via `Arc`.
    memory: crate::conversation::state::memory::MemoryOf,
    /// The session/conversation id this hook compacts.
    conversation_id: String,
    /// Hook-driven compaction machinery: compactor, policy, threshold.
    compaction: CompactionConfig,
    /// The wire serving this turn's completions. The pricing fallback uses its
    /// spec, so a completion is priced at the rates of the model that served
    /// it — not at the compaction model's.
    model: rig::DynModel<rig::operation::Completion>,
    /// Real token count from the last LLM completion, shared across turns. Used by
    /// `decide_compaction` to check the threshold against the real context size
    /// (plus the current prompt) instead of the chars/4 heuristic. `None` before
    /// the first completion and after a compaction reset.
    last_total_tokens: Arc<Mutex<Option<u64>>>,
    /// Model ids already reported as unpriced, so the warn fires once per
    /// model rather than once per call.
    unpriced_warnings: UnpricedWarnings,
}

impl<P: AsyncPermissionResolver> HookChain<P> {
    pub fn new(
        bus: Bus,
        permission_resolver: P,
        closure_registry: Arc<ClosureRegistry>,
        mcp_registry: Arc<McpToolRegistry>,
        render_registry: ToolRenderRegistry,
        max_tool_calls_per_subturn: Option<usize>,
        hook_state: HookState,
    ) -> Self {
        Self {
            cancel_rx: Arc::new(Mutex::new(bus.cancel().subscribe())),
            subturn: SubTurnCap::new(
                max_tool_calls_per_subturn.unwrap_or(defaults::MAX_TOOL_CALLS_PER_SUBTURN),
            ),
            doom: DoomLoopDetector {
                state: hook_state.doom_state,
            },
            output_repetition: hook_state.output_repetition,
            repetition_guard: hook_state.repetition_guard,
            circuit: CircuitBreakerGuard {
                breaker: hook_state.circuit_breaker,
            },
            history: HistorySnapshot::default(),
            permission: permission_resolver,
            bus,
            closure_registry,
            mcp_registry,
            render_registry,
            memory: hook_state.memory,
            conversation_id: hook_state.conversation_id,
            compaction: hook_state.compaction,
            model: hook_state.model,
            last_total_tokens: hook_state.last_total_tokens,
            unpriced_warnings: UnpricedWarnings::default(),
        }
    }

    /// Return a clone of the `Arc` that holds the most recent history snapshot.
    ///
    /// Callers clone this Arc **before** passing the hook into the agent builder
    /// (which consumes `self`), then read it back after a `ProviderError`.
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

    /// The warning for a model nothing priced, or `None` when this model was
    /// already reported.
    ///
    /// The model named is the wire serving this turn's completions, so the log
    /// names the model that actually went unpriced.
    fn unpriced_warning(&self, reason: PricingSkip) -> Option<String> {
        let model = &self.model;
        let model_id = model.id().unwrap_or("<no model>").to_string();
        let vendor = model.name().to_string();
        if !self.unpriced_warnings.first_time(&model_id) {
            return None;
        }
        Some(format!(
            "no cost for {vendor}/{model_id}: {reason} — the session cost line stays tokens-only for this model"
        ))
    }

    /// Report an unpriced completion once per model id.
    ///
    /// A tokens-only session used to be unexplainable: nothing said why no
    /// cost appeared. The reason is logged at warn level the first time a
    /// model goes unpriced, so the log names the cause without repeating it
    /// on every call.
    fn warn_unpriced_once(&self, reason: PricingSkip) {
        if let Some(message) = self.unpriced_warning(reason) {
            log::warn!("{message}");
        }
    }
}

impl<P: AsyncPermissionResolver> AgentHook for HookChain<P> {
    fn on_model_select(
        &self,
        _ctx: &HookContext,
        _event: ModelSelection<'_>,
    ) -> ModelSelectionAction {
        // The executor builds a fresh agent per turn with the current model
        // registered as the default, so keeping the default candidate is the
        // correct selection. Label-based switching is subtask 506682ea.
        ModelSelectionAction::Continue
    }

    fn on_completion_call(
        &self,
        _ctx: &HookContext,
        event: CompletionCallEvent<'_>,
    ) -> impl std::future::Future<Output = CompletionCallAction> + WasmCompatSend {
        // 1. History snapshot — store history + prompt before the LLM call
        let history: Vec<Message> = event.history.iter().map(|m| (**m).clone()).collect();
        self.history.update(&history, event.prompt);

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
        let prompt = event.prompt.clone();

        async move {
            if cancelled {
                return CompletionCallAction::stop("Cancelled by user");
            }
            let _ = bus.ui_event().send(UiEvent::LlmStarted).await;

            // Compaction decision: schedule a compaction when the session's
            // current conversation is over the threshold. The hook never patches
            // the per-turn history — the splice is what the next run loads.
            decide_compaction(
                &prompt,
                conversation_id.as_str(),
                memory.as_ref(),
                &compaction,
                &last_total_tokens,
                &bus,
            )
            .await;

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
        let delta = event.delta.to_string();
        let text = event.aggregated.to_string();
        // Intra-stream repetition detection: check the full aggregated stream
        // text (not individual deltas — streaming servers tokenize differently
        // each pass, so delta chunks are never reliably identical, but the
        // repetition is fully visible in the aggregated suffix). The 100-char
        // growth throttle inside `check_aggregated` bounds the check cost.
        // Compute it synchronously before the `async move` so `event` is not
        // borrowed across the await point. Skipped entirely when the repetition
        // guard is disabled.
        let repetition_action = if self.repetition_guard {
            OutputRepetitionDetector::check_aggregated(&self.output_repetition, event.aggregated)
        } else {
            None
        };

        async move {
            if cancelled {
                return ObservationAction::stop("Cancelled by user");
            }
            // Every intra-stream detection stops the stream: `check_aggregated`
            // returns Stop on ANY detection (the stream stops on the FIRST,
            // not after ladder escalations). NO warning here — the executor
            // surfaces the stop reason and drives the stop-to-steering retry
            // (append steering + reset ladder + re-run, terminal on the second
            // stop), the same path as the Esc-Esc cancel: `ObservationAction
            // ::stop` → Cancelled → cancel-reason matching →
            // `LlmEvent::Stopped` + `WarningEvent`.
            if repetition_action.is_some() {
                return ObservationAction::stop(format!(
                    "{OUTPUT_REPETITION_STOP_PREFIX} the assistant kept repeating the same \
                     output after repeated steering. The run was stopped."
                ));
            }
            let _ = bus
                .ui_event()
                .send(UiEvent::AssistantMessage { delta, text })
                .await;
            ObservationAction::continue_run()
        }
    }

    fn on_dispatch(
        &self,
        _ctx: &HookContext,
        event: DispatchEvent<'_>,
    ) -> impl std::future::Future<Output = DispatchAction> + WasmCompatSend {
        // `on_dispatch` is unified: it fires for tool calls and completions.
        // Only tool calls are steered here; a completion dispatch proceeds
        // without consuming any per-tool state (including the cancel signal).
        let tool_name = event.tool_name().map(str::to_string);
        let args = event.tool_args().map(str::to_string);
        let tool_call_id = event.call_id.map(|id| id.to_string());

        // Pre-compute all synchronous checks and capture values for the async
        // block. Each is guarded on `tool_name` so a completion dispatch
        // neither consumes the cancel event nor advances the sub-turn cap.
        let cancelled = tool_name.is_some() && self.is_cancelled();
        let subturn_action = tool_name
            .as_deref()
            .and_then(|name| self.subturn.check_and_increment(name));
        let circuit_action = tool_name
            .as_deref()
            .and_then(|name| self.circuit.check_server_enabled(name, &self.mcp_registry));
        let source = tool_name
            .as_deref()
            .map(|name| {
                resolve_tool_source(name, &self.closure_registry, &self.mcp_registry)
                    .as_str()
                    .to_string()
            })
            .unwrap_or_default();

        let permission = self.permission.clone();
        let bus = self.bus.clone();

        async move {
            let Some(tool_name) = tool_name else {
                return DispatchAction::proceed();
            };
            let args = args.unwrap_or_default();

            log::trace!("on_dispatch: tool={tool_name}");

            if cancelled {
                return DispatchAction::stop("Cancelled by user");
            }
            if let Some(action) = subturn_action {
                return action;
            }
            let doom_action = if self.repetition_guard {
                self.doom.check_and_record(&tool_name, &args, &bus).await
            } else {
                None
            };
            if let Some(action) = doom_action {
                return action;
            }
            if let Some(action) = circuit_action {
                return action;
            }

            let call_line = self.render_registry.render(&tool_name, &args);
            let _ = bus
                .ui_event()
                .send(UiEvent::ToolStarted {
                    name: tool_name.clone(),
                    source: source.clone(),
                    arguments: args.clone(),
                    call_line,
                })
                .await;

            let decision = permission
                .resolve(&tool_name, &args, tool_call_id, &bus)
                .await;
            match decision {
                PermissionDecision::Allow => DispatchAction::proceed(),
                PermissionDecision::Deny { reason } => {
                    // The tool never runs, so on_outcome never fires to
                    // consume a previously-recorded preview flag — discard it
                    // here so it doesn't linger.
                    permission.take_previewed(&tool_name, &args);
                    let _ = bus
                        .ui_event()
                        .send(UiEvent::ToolCompleted {
                            name: tool_name,
                            source,
                            arguments: args,
                            success: false,
                            result: String::new(),
                            display: None,
                            error_kind: None,
                            message: Some(reason.clone()),
                        })
                        .await;
                    DispatchAction::skip(reason)
                }
            }
        }
    }

    fn on_outcome(
        &self,
        _ctx: &HookContext,
        event: OutcomeEvent<'_>,
    ) -> impl std::future::Future<Output = OutcomeAction> + WasmCompatSend {
        // `on_outcome` is unified: it fires for tool results and completions.
        // A tool result drives the transcript event, the circuit breaker, and
        // the persisted success verdict; a completion drives usage reporting.
        // Both proceed.
        let bus = self.bus.clone();

        async move {
            // Tool branch: structural success, persisted verdict, transcript
            // event, and circuit-breaker tracking.
            if let Some(result) = event.tool_result() {
                let tool_name = event.tool_name().unwrap_or_default();
                let args = event.tool_args().unwrap_or_default();
                let result_text = result.output().render();

                // Structural success: the canonical result disposition decides. A
                // non-zero `nu` exit arrives as an Error disposition (producers are
                // honest), and Refused/Skipped never count as success — no result-text
                // sniffing.
                let success = result.is_success();

                log::trace!(
                    "on_outcome: tool={tool_name} success={success} result_len={}",
                    result_text.len(),
                );
                if log::log_enabled!(log::Level::Trace) {
                    let preview = if result_text.len() > 2000 {
                        let boundary = result_text.floor_char_boundary(2000);
                        format!(
                            "{}...<truncated {} bytes>",
                            &result_text[..boundary],
                            result_text.len()
                        )
                    } else {
                        result_text.clone()
                    };
                    log::trace!("  result_body: {preview}");
                }

                // 1. Parse result JSON and extract display
                let display = serde_json::from_str::<serde_json::Value>(&result_text)
                    .ok()
                    .and_then(|json| {
                        crate::tools::handler::tool_display_from_result(tool_name, &json)
                    });

                // Suppress the display if a pre-authorize preview of it was already
                // shown to the user before the tool ran (the Ask-path edit diff).
                // `take_previewed` is the single, synchronous source of truth for
                // this — resolved deterministically inside the same before/after
                // hook call chain, so there is no event-ordering race to get wrong.
                let display =
                    suppress_previewed_display(&self.permission, tool_name, args, display);

                // 2. Resolve source
                let source =
                    resolve_tool_source(tool_name, &self.closure_registry, &self.mcp_registry)
                        .as_str()
                        .to_string();

                // 3. Classify error kind from raw result
                let error_kind = result.error().map(|e| e.kind().as_str().to_string());

                // 4. Emit ToolCompleted (success was computed structurally above)
                let _ = bus
                    .ui_event()
                    .send(UiEvent::ToolCompleted {
                        name: tool_name.to_string(),
                        source,
                        arguments: args.to_string(),
                        success,
                        result: result_text,
                        display,
                        error_kind,
                        message: None,
                    })
                    .await;

                // 5. Circuit breaker — track transport failures per server
                self.circuit
                    .record_result(tool_name, result, success, &self.mcp_registry, &bus)
                    .await;
            }

            // Completion branch: report usage and store the real token count for
            // the compaction threshold check.
            if let Some(response) = event.completion() {
                let usage = response.usage;
                // Any counter is enough to report the call: a wire may omit
                // `total_tokens` (ollama-cloud's OpenAI-compatible endpoint
                // does), and pricing needs only the input and output counts.
                // A usage that reports no counter at all reports nothing.
                let reports_usage = usage.input_tokens.is_some()
                    || usage.output_tokens.is_some()
                    || usage.total_tokens.is_some();
                if reports_usage {
                    let mut response_chars = 0usize;
                    let mut tool_calls = 0usize;
                    for item in response.choice.iter() {
                        match item {
                            rig::message::AssistantContent::Text(text) => {
                                response_chars += text.text.chars().count();
                            }
                            rig::message::AssistantContent::ToolCall(_) => tool_calls += 1,
                            _ => {}
                        }
                    }
                    // Store the real count: the wire's total when it reports
                    // one, else input + output — the same quantity. A wire may
                    // omit `total_tokens` (ollama-cloud's OpenAI-compatible
                    // endpoint does), and the switch gate and the threshold
                    // check both need a real count.
                    // Mutex poison is a fatal internal inconsistency — panicking is correct.
                    let total_tokens =
                        match (usage.total_tokens, usage.input_tokens, usage.output_tokens) {
                            (Some(total), _, _) => Some(total),
                            (None, Some(input), Some(output)) => Some(input + output),
                            _ => None,
                        };
                    if let Some(total_tokens) = total_tokens {
                        *self.last_total_tokens.lock().unwrap() = Some(total_tokens);
                    }
                    let (cost, cost_complete) = match usage.cost {
                        Some(cost) => (Some(cost.total), cost.is_complete()),
                        None => match computed_cost(&self.model, &usage) {
                            PricedOutcome::Priced { total, complete } => (Some(total), complete),
                            PricedOutcome::Skipped(reason) => {
                                self.warn_unpriced_once(reason);
                                (None, false)
                            }
                        },
                    };
                    let _ = bus
                        .ui_event()
                        .send(UiEvent::LlmCompleted {
                            response_chars,
                            tool_calls,
                            input_tokens: usage.input_tokens.unwrap_or(0),
                            output_tokens: usage.output_tokens.unwrap_or(0),
                            total_tokens: usage.total_tokens,
                            cost,
                            cost_complete,
                        })
                        .await;
                }
            }

            OutcomeAction::proceed()
        }
    }

    fn on_model_turn_finished(
        &self,
        _ctx: &HookContext,
        event: ModelTurnFinished<'_>,
    ) -> impl std::future::Future<Output = ModelTurnAction> + WasmCompatSend {
        // Detect assistant-output repetition at the turn boundary. The action
        // must be computable before the `async move` so it is decided
        // synchronously. `finish_turn` consumes the intra-stream detection
        // flag (escalating the ladder when intra-stream detection fired) and
        // otherwise falls back to the cross-turn full-text comparison.
        // Skipped entirely when the repetition guard is disabled.
        let repetition_detection = if self.repetition_guard {
            OutputRepetitionDetector::finish_turn(&self.output_repetition, event.content)
        } else {
            None
        };
        let bus = self.bus.clone();
        async move {
            if let Some(detection) = repetition_detection {
                match detection {
                    RepetitionDetection::First(message) | RepetitionDetection::Backoff(message) => {
                        let _ = bus
                            .ui_event()
                            .send(UiEvent::Warning {
                                message: message.clone(),
                            })
                            .await;
                        return ModelTurnAction::retry_with_feedback(message);
                    }
                    RepetitionDetection::Stop(message) => {
                        // No hook-side warning — the executor surfaces the stop
                        // reason, mirroring the tool doom loop.
                        return ModelTurnAction::stop(message);
                    }
                }
            }
            ModelTurnAction::continue_run()
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
                .ui_event()
                .send(UiEvent::Warning {
                    message: feedback.clone(),
                })
                .await;
            Some(InvalidToolCallAction::retry(feedback))
        }
    }
}

// region: --- Compaction

/// Decide whether this turn should schedule a compaction.
///
/// The threshold is evaluated against the CURRENT conversation of the session's
/// rig-memory store — the store a run loads at its start — so a turn saved after
/// the cache was populated is counted. `rig_memory::load` re-reads and folds the
/// log on every call, so it is always current; the first-read cache is not a
/// compaction input.
///
/// The hook never patches the per-turn history: after a compaction the splice is
/// saved and rig loads it at the next run.
async fn decide_compaction(
    prompt: &Message,
    conversation_id: &str,
    memory: &crate::session::CachedMemory,
    compaction: &CompactionConfig,
    last_total_tokens: &Arc<Mutex<Option<u64>>>,
    bus: &Bus,
) {
    let Some(threshold_tokens) = compaction.threshold_tokens else {
        return;
    };
    // A session whose provider name resolved to no rig provider id has no model
    // spec, so its compactor cannot run: scheduling one would only emit a
    // misleading `Requested`.
    if compaction.spec().is_none() {
        return;
    }

    let Some(conversation) = current_conversation(memory, conversation_id, bus).await else {
        return;
    };
    if conversation.is_empty() {
        return;
    }

    // Threshold check: use the real token count from the last completion when
    // available (it reflects the actual context the model processed last turn,
    // which already includes the prior prompt), plus a chars/4 estimate for the
    // current prompt. When no real count exists (first turn, or after a
    // compaction reset) fall back to the chars/4 estimate for the conversation.
    // Mutex poison is a fatal internal inconsistency — panicking is correct.
    let base_tokens = last_total_tokens
        .lock()
        .unwrap()
        .and_then(|n| u64::try_into(n).ok())
        .unwrap_or_else(|| estimate_token_count(&conversation));
    let prompt_estimate = estimate_token_count(std::slice::from_ref(prompt));
    if base_tokens + prompt_estimate < threshold_tokens {
        return;
    }

    // Compaction is needed. Fire a `CompactionEvent::Requested` on the bus so the
    // orchestrator schedules the compaction on the worker; the splice it makes is
    // what the next run loads.
    let _ = bus
        .compaction()
        .send(CompactionEvent::Requested {
            source: "auto".to_string(),
        })
        .await;
}

/// The conversation the session's rig-memory store holds right now.
///
/// Store errors are surfaced as a `CompactionEvent::Failed` on `bus` and yield
/// `None` so the turn proceeds without a compaction decision.
async fn current_conversation(
    memory: &crate::session::CachedMemory,
    conversation_id: &str,
    bus: &Bus,
) -> Option<Vec<Message>> {
    let store = match memory.rig_store(conversation_id) {
        Ok(store) => store,
        Err(e) => {
            let _ = bus
                .compaction()
                .send(CompactionEvent::Failed {
                    source: "auto".to_string(),
                    message: format!("Failed to open the session store: {e}"),
                })
                .await;
            return None;
        }
    };
    match rig_memory::load(&*store, conversation_id) {
        Ok(conversation) => Some(
            conversation
                .messages()
                .iter()
                .map(|message| (**message).clone())
                .collect(),
        ),
        Err(e) => {
            let _ = bus
                .compaction()
                .send(CompactionEvent::Failed {
                    source: "auto".to_string(),
                    message: format!("Failed to load the session: {e}"),
                })
                .await;
            None
        }
    }
}

/// Run a compaction for `conversation_id`, returning the post-splice history
/// when compaction ran, or `None` when there was nothing to compact.
///
/// Called by the router when the orchestrator dispatches a `RunCompaction`
/// worker command. `source` is `"auto"` or `"slash"`. `compact()` emits all
/// `Started`/`SummaryChunk`/`Completed`/`Failed` events; this function emits a
/// `Completed` with an empty summary when there was nothing to summarize, so the
/// TUI still gets feedback that the `/compact` command ran.
///
/// The conversation is read from the rig-memory store of the session, which is
/// what a run loads at its start: the splice `compact()` makes IS the context
/// from then on.
pub(crate) async fn run_compaction(
    conversation_id: &str,
    memory: &crate::session::CachedMemory,
    compaction: &CompactionConfig,
    source: &str,
    last_total_tokens: &Arc<Mutex<Option<u64>>>,
    bus: &Bus,
) -> Option<Vec<Message>> {
    let store = match memory.rig_store(conversation_id) {
        Ok(store) => store,
        Err(e) => {
            let _ = bus
                .compaction()
                .send(CompactionEvent::Failed {
                    source: source.to_string(),
                    message: format!("Failed to open the session store: {e}"),
                })
                .await;
            return None;
        }
    };

    let reason = compact_reason(source);
    match compaction
        .compactor
        .compact(&*store, conversation_id, &reason, source)
        .await
    {
        Ok(Some(outcome)) => {
            // Compaction succeeded: the real token count from the previous turn no
            // longer reflects the (now summarized) context. Reset it so the next
            // turn falls back to the estimate (small — just the summary), then
            // picks up the real count after its first completion. Mutex poison is
            // fatal — panicking is correct.
            *last_total_tokens.lock().unwrap() = None;
            Some(outcome.messages)
        }
        Ok(None) => {
            let _ = bus
                .compaction()
                .send(CompactionEvent::Completed {
                    source: source.to_string(),
                    summary_preview: String::new(),
                    summary_body: String::new(),
                })
                .await;
            None
        }
        // `compact()` already emitted `Failed`; fall back to the full history so
        // the turn is not lost.
        Err(_) => None,
    }
}

/// Why a compaction is running, from the command that asked for it.
///
/// `"slash"` is the user asking through `/compact`; anything else is the
/// automatic path.
fn compact_reason(source: &str) -> CompactReason {
    if source == "slash" {
        CompactReason::Asked {
            focus: String::new(),
        }
    } else {
        CompactReason::Threshold
    }
}

/// Decide whether a tool-completion `display` should still be attached, or
/// suppressed because a pre-authorize preview of it was already shown to the
/// user before the tool ran.
///
/// Pulled out as a pure function (rather than inlined in `on_outcome`'s
/// async block) so it's directly unit-testable against a stub resolver,
/// without needing rig's opaque `OutcomeEvent` type.
fn suppress_previewed_display<P: crate::hook::permission_resolver::AsyncPermissionResolver>(
    permission: &P,
    tool_name: &str,
    args: &str,
    display: Option<crate::protocol::event::ToolDisplay>,
) -> Option<crate::protocol::event::ToolDisplay> {
    if permission.take_previewed(tool_name, args) {
        None
    } else {
        display
    }
}

// endregion: --- Compaction

#[cfg(test)]
#[path = "../../test/hook/chain.rs"]
mod chain_test;
