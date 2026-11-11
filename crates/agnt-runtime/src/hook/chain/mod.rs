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

// region:    --- Modules

mod compaction;
mod dispatch;
mod events;
mod hook_impl;
mod not_run;
mod outcome;

pub use compaction::{CompactionPath, run_compaction};
pub use not_run::{
    CANCELLED_DISPATCH_MESSAGE, CANCELLED_STOP_REASON, NotRunCall, NotRunOutcome, NotRunStart,
    emit_not_run_dispatch,
};

// endregion: --- Modules

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agnt_kernel::bus::CancelRx;

use crate::bus::Bus;
use crate::conversation::compaction::CompactionConfig;
use crate::hook::turn_tools::TurnToolLog;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;

use super::agent_hook::StopLedger;
use super::circuit_breaker_guard::CircuitBreakerGuard;
use super::doom_loop::DoomLoopDetector;
use super::history_snapshot::HistorySnapshot;
use super::output_repetition::RepetitionState;
use super::permission_resolver::AsyncPermissionResolver;
use super::subturn_cap::SubTurnCap;
use super::turn_usage::TurnUsage;
use super::unpriced_warnings::UnpricedWarnings;

// region:    --- Types

/// The environment a hook chain publishes into and resolves against.
///
/// Groups the bus and the session working directory so `HookChain::new` stays
/// under clippy's `too_many_arguments` threshold.
#[derive(Clone)]
pub struct HookEnv {
    /// Shared signal bus — tool/LLM/warning events are published here.
    pub bus: Bus,
    /// The session working directory, read to build the argument-sourced
    /// preview for a call the turn stopped before it ran.
    pub cwd: PathBuf,
}

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
    /// Every tool call this turn dispatched and the result each call settled
    /// with. Read after the turn so a stop keeps the in-flight batch that
    /// rig's history drops.
    turn_tools: Arc<TurnToolLog>,
    permission: P,
    /// Shared signal bus — tool/LLM/warning events are published here.
    bus: Bus,
    closure_registry: Arc<ClosureRegistry>,
    mcp_registry: Arc<McpToolRegistry>,
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
    /// The turn's LLM usage, accumulated as each call completes. The turn
    /// executor reads the sums when it publishes the turn-completed event.
    turn_usage: TurnUsage,
    /// The session's cumulative LLM usage. Every call adds into it; it resets
    /// only when the runtime switches sessions.
    session_usage: TurnUsage,
    /// The store-dead latch, shared with the turn's tool infrastructure: the
    /// clean refusal text once the session store could not be opened. While it
    /// is set, the compaction decision opens no store, so an unreadable store
    /// costs one line, not one per turn.
    store_latch: crate::hook::StoreLatch,
    /// Model ids already reported as unpriced, so the warn fires once per
    /// model rather than once per call.
    unpriced_warnings: UnpricedWarnings,
    /// Why this turn's run stopped, recorded when a guard or the user stops it.
    stop: StopLedger,
}

// endregion: --- Types

#[cfg(test)]
#[path = "../../../test/hook/chain.rs"]
mod chain_test;
