//! `HookChain` construction, its accessors, and the `AgentHook` impl.
//!
//! The trait method bodies live in the module that owns the work: the model-side
//! events in `events`, the dispatch gate in `dispatch`, the outcome in `outcome`.
//! This module holds the wiring and the per-chain state helpers.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rig::agent::{
    AgentHook, CompletionCallAction, CompletionCallEvent, DispatchAction, DispatchEvent,
    HookContext, InvalidToolCallAction, InvalidToolCallContext, ModelSelection,
    ModelSelectionAction, ModelTurnAction, ModelTurnFinished, ObservationAction, OutcomeAction,
    OutcomeEvent, TextDelta,
};
use rig::message::Message;
use rig::wasm_compat::WasmCompatSend;

use agnt_kernel::bus::{CancelEvent, CancelRx, TryRecvError};

use crate::config::defaults;
use crate::hook::agent_hook::{HookState, StopLedger};
use crate::hook::circuit_breaker_guard::CircuitBreakerGuard;
use crate::hook::doom_loop::DoomLoopDetector;
use crate::hook::history_snapshot::HistorySnapshot;
use crate::hook::permission_resolver::AsyncPermissionResolver;
use crate::hook::subturn_cap::SubTurnCap;
use crate::hook::turn_tools::TurnToolLog;
use crate::hook::unpriced_warnings::UnpricedWarnings;
use crate::llm::PricingSkip;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;

use super::not_run::{NotRunCall, NotRunOutcome, NotRunStart, emit_not_run_dispatch};
use super::{HookChain, HookEnv};

impl<P: AsyncPermissionResolver> HookChain<P> {
    pub fn new(
        env: HookEnv,
        permission_resolver: P,
        closure_registry: Arc<ClosureRegistry>,
        mcp_registry: Arc<McpToolRegistry>,
        max_tool_calls_per_subturn: Option<usize>,
        hook_state: HookState,
    ) -> Self {
        let cancel_rx = Arc::new(Mutex::new(env.bus.cancel().subscribe()));
        Self {
            cancel_rx,
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
            turn_tools: Arc::new(TurnToolLog::default()),
            permission: permission_resolver,
            bus: env.bus,
            closure_registry,
            mcp_registry,
            memory: hook_state.memory,
            conversation_id: hook_state.conversation_id,
            compaction: hook_state.compaction,
            model: hook_state.model,
            last_total_tokens: hook_state.last_total_tokens,
            turn_usage: hook_state.turn_usage,
            session_usage: hook_state.session_usage,
            store_latch: hook_state.store_latch,
            unpriced_warnings: UnpricedWarnings::default(),
            stop: StopLedger::default(),
        }
    }

    /// Return a clone of the `Arc` that holds the most recent history snapshot.
    ///
    /// Callers clone this Arc **before** passing the hook into the agent builder
    /// (which consumes `self`), then read it back after a `ProviderError`.
    pub fn last_known_history(&self) -> std::sync::Arc<std::sync::Mutex<Vec<Message>>> {
        self.history.arc()
    }

    /// Return a clone of the `Arc` that holds this turn's tool records.
    ///
    /// Callers clone this Arc **before** passing the hook into the agent
    /// builder (which consumes `self`), then read it back after the turn —
    /// including after a stop, whose dropped stream leaves the in-flight batch
    /// out of rig's history.
    pub fn turn_tools(&self) -> Arc<TurnToolLog> {
        Arc::clone(&self.turn_tools)
    }

    /// Return a clone of the ledger that records why the run stopped.
    ///
    /// Callers clone it **before** passing the hook into the agent builder
    /// (which consumes `self`), then read it after the run: a hook stop carries
    /// its reason text through rig, so the cause travels beside it here.
    pub fn stop_ledger(&self) -> StopLedger {
        self.stop.clone()
    }

    /// Returns `true` if a cancellation has been requested via the bus cancel channel.
    ///
    /// Consumes any pending cancel event. A `Lagged` error also counts as cancelled
    /// because the cancel channel is capacity-bounded and an overflowed buffer means
    /// the turn must stop.
    pub(super) fn is_cancelled(&self) -> bool {
        let mut rx = self.lock_cancel_rx();
        matches!(
            rx.try_recv(),
            Ok(CancelEvent::Requested) | Err(TryRecvError::Lagged(_))
        )
    }

    /// The cancel receiver, recovered if a drainer panicked mid-receive.
    fn lock_cancel_rx(&self) -> MutexGuard<'_, CancelRx> {
        self.cancel_rx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The last completion's token count, recovered if a writer panicked.
    pub(super) fn lock_last_total_tokens(&self) -> MutexGuard<'_, Option<u64>> {
        self.last_total_tokens
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Publish the not-run sequence for a call the cancel arm denied before it
    /// ran.
    ///
    /// The cancel arm returns `stop` before the `ToolStarted` send, and rig
    /// fails the dispatch before `on_outcome` fires — so without this sequence
    /// the live transcript renders nothing for the denied call.
    pub(super) async fn emit_cancelled_dispatch(
        &self,
        tool_name: &str,
        source: crate::tools::handler::ToolSource,
        arguments: &str,
    ) {
        emit_not_run_dispatch(
            self.bus.stream(),
            NotRunCall {
                tool_name,
                source,
                arguments,
                start: NotRunStart::Publish,
            },
            NotRunOutcome::Cancelled,
        )
        .await;
    }

    /// The warning for a model nothing priced, or `None` when this model was
    /// already reported.
    ///
    /// The model named is the wire serving this turn's completions, so the log
    /// names the model that actually went unpriced.
    pub(super) fn unpriced_warning(&self, reason: PricingSkip) -> Option<String> {
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
    pub(super) fn warn_unpriced_once(&self, reason: PricingSkip) {
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
        ctx: &HookContext,
        event: CompletionCallEvent<'_>,
    ) -> impl std::future::Future<Output = CompletionCallAction> + WasmCompatSend {
        self.begin_completion(ctx, event)
    }

    fn on_text_delta(
        &self,
        ctx: &HookContext,
        event: TextDelta<'_>,
    ) -> impl std::future::Future<Output = ObservationAction> + WasmCompatSend {
        self.stream_delta(ctx, event)
    }

    fn on_dispatch(
        &self,
        ctx: &HookContext,
        event: DispatchEvent<'_>,
    ) -> impl std::future::Future<Output = DispatchAction> + WasmCompatSend {
        self.gate_dispatch(ctx, event)
    }

    fn on_outcome(
        &self,
        ctx: &HookContext,
        event: OutcomeEvent<'_>,
    ) -> impl std::future::Future<Output = OutcomeAction> + WasmCompatSend {
        self.settle_outcome(ctx, event)
    }

    fn on_model_turn_finished(
        &self,
        ctx: &HookContext,
        event: ModelTurnFinished<'_>,
    ) -> impl std::future::Future<Output = ModelTurnAction> + WasmCompatSend {
        self.check_turn_repetition(ctx, event)
    }

    fn on_invalid_tool_call(
        &self,
        ctx: &HookContext,
        event: &InvalidToolCallContext,
    ) -> impl std::future::Future<Output = Option<InvalidToolCallAction>> + WasmCompatSend {
        self.reject_invalid_tool_call(ctx, event)
    }
}
