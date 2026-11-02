//! AgentHook — transition shim.
//!
//! All implementation lives in the concern modules and `chain.rs`.

use std::sync::{Arc, Mutex};

use agnt_kernel::{InferenceRecord, SessionRecord, StopCause};

use crate::bus::Bus;
use crate::conversation::compaction::CompactionConfig;
use crate::conversation::state::memory::MemoryOf;
use crate::tools::mcp::circuit_breaker::McpCircuitBreaker;

pub use crate::hook::chain::HookChain;
pub use crate::hook::doom_loop::{DOOM_LOOP_THRESHOLD, DoomLoopState};
pub use crate::hook::output_repetition::RepetitionState;
pub use crate::hook::permission_resolver::{AsyncPermissionResolver, PermissionDecision};
pub use crate::hook::turn_usage::TurnUsage;

/// The last clean store refusal, shared by the hook chain and the turn's
/// tool infrastructure: `None` while the session store opens, and the refusal
/// text once it could not, so no further per-turn attempt is made.
pub type StoreLatch = Arc<Mutex<Option<String>>>;

/// Why the turn's run stopped, recorded by the hook that decided to stop.
///
/// A hook stop carries its reason text through rig, and text is content: the
/// cause travels beside it as data, recorded here by the producer that acted on
/// it. The turn executor reads it to narrate the stop and to decide whether a
/// repetition stop becomes a steering retry — neither consumer parses the text.
#[derive(Clone, Default)]
pub struct StopLedger {
    cause: Arc<Mutex<Option<StopCause>>>,
}

impl StopLedger {
    /// Record the cause. The first stop of a turn wins: a stopped stream ends
    /// the run, so a later record would describe work that never happened.
    pub fn record(&self, cause: StopCause) {
        // Mutex poison is a fatal internal inconsistency — panicking is correct.
        let mut slot = self.cause.lock().unwrap();
        if slot.is_none() {
            *slot = Some(cause);
        }
    }

    /// Take the recorded cause, leaving the ledger empty.
    pub fn take(&self) -> Option<StopCause> {
        // Mutex poison is a fatal internal inconsistency — panicking is correct.
        self.cause.lock().unwrap().take()
    }

    /// Record the stop and narrate it on the record stream.
    ///
    /// One place, so a stop's cause and its record can never disagree. A failed
    /// publish means no subscriber is attached (zero retention), not an error.
    pub fn narrate(&self, bus: &Bus, cause: StopCause, reason: &str) {
        self.record(cause);
        let _ = bus
            .stream()
            .publish(SessionRecord::Inference(InferenceRecord::Stopped {
                cause,
                reason: reason.to_string(),
            }));
    }
}

/// Session-scoped state shared across turns via the hook.
///
/// Bundles the `Arc<Mutex<…>>` state that outlives individual turns and
/// must accumulate across them (circuit breaker failures, doom loop
/// signatures), plus the memory backend, compactor, compaction policy, and
/// force-compact flag used by the hook's compaction logic.
#[derive(Clone)]
pub struct HookState {
    pub circuit_breaker: Arc<Mutex<McpCircuitBreaker>>,
    pub doom_state: Arc<Mutex<DoomLoopState>>,
    /// Session-scoped assistant-output repetition state, shared across turns
    /// via the hook. Survives caller retries so consecutive text-only
    /// completions accumulate across executor attempts.
    pub output_repetition: Arc<Mutex<RepetitionState>>,
    /// Enable/disable the repetition guard (doom-loop + output-repetition
    /// detection). When `false`, the hook chain skips all detector calls.
    pub repetition_guard: bool,
    /// Memory backing the conversation (shared with the turn executor).
    pub memory: MemoryOf,
    /// The session/conversation id this hook compacts.
    pub conversation_id: String,
    /// Hook-driven compaction machinery: compactor, policy, force flag, threshold.
    pub compaction: CompactionConfig,
    /// The wire serving this turn's completions. The pricing fallback uses its
    /// spec, so a completion is priced at the rates of the model that served
    /// it — not at the compaction model's.
    pub model: rig::DynModel<rig::operation::Completion>,
    /// Real token count from the last LLM completion: the wire's total when it
    /// reports one, else input + output. Shared across turns via the hook.
    /// `None` before the first completion and after a compaction reset; the
    /// compaction threshold falls back to the chars/4 estimate when `None`.
    pub last_total_tokens: Arc<Mutex<Option<u64>>>,
    /// The turn's LLM usage, accumulated as each call completes. Created per
    /// turn, so the sums never carry over into the next one; the turn executor
    /// reads them when it publishes the turn-completed event.
    pub turn_usage: TurnUsage,
    /// The session's cumulative LLM usage. Shared across turns; the chain adds
    /// every call into it, and it resets only when the runtime switches
    /// sessions.
    pub session_usage: TurnUsage,
    /// The store-dead latch: the clean refusal text once the session store
    /// could not be opened, `None` while it opens. Shared with the turn's
    /// `ToolInfra` so a session change can clear it.
    pub store_latch: StoreLatch,
}

#[cfg(test)]
#[path = "../../test/hook/agent_hook.rs"]
mod agent_hook_test;
