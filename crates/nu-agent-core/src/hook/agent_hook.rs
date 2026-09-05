//! AgentHook — transition shim.
//!
//! All implementation lives in the concern modules and `chain.rs`.

use std::sync::{Arc, Mutex};

use crate::conversation::compaction::CompactionConfig;
use crate::conversation::state::memory::MemoryOf;
use crate::session::SessionStore;
use crate::tools::mcp::circuit_breaker::McpCircuitBreaker;

pub use crate::hook::chain::HookChain;
pub use crate::hook::doom_loop::{DOOM_LOOP_THRESHOLD, DoomLoopState};
pub use crate::hook::permission_resolver::{AsyncPermissionResolver, PermissionDecision};

/// Session-scoped state shared across turns via the hook.
///
/// Bundles the `Arc<Mutex<…>>` state that outlives individual turns and
/// must accumulate across them (circuit breaker failures, doom loop
/// signatures), plus the memory backend, compactor, compaction policy, and
/// force-compact flag used by the hook's compaction logic.
#[derive(Clone)]
pub struct HookState<S: SessionStore + Clone + Send + Sync> {
    pub circuit_breaker: Arc<Mutex<McpCircuitBreaker>>,
    pub doom_state: Arc<Mutex<DoomLoopState>>,
    /// Shared runtime model handle. The single point of model identity: the
    /// hook's `on_model_select` routes each turn to its current value. It is
    /// constructed eagerly at startup and updated on every `switch_model()`.
    pub shared_model: Arc<Mutex<rig::agent::ModelHandle>>,
    /// Memory backing the conversation (shared with the turn executor).
    pub memory: MemoryOf<S>,
    /// The session/conversation id this hook compacts.
    pub conversation_id: String,
    /// Hook-driven compaction machinery: compactor, policy, force flag, threshold.
    pub compaction: CompactionConfig<S>,
    /// Real token count from the last LLM completion (from `Usage.total_tokens`),
    /// shared across turns via the hook. `None` before the first completion and
    /// after a compaction reset; the compaction threshold falls back to the
    /// chars/4 estimate when `None`.
    pub last_total_tokens: Arc<Mutex<Option<u64>>>,
}

#[cfg(test)]
#[path = "agent_hook_test.rs"]
mod agent_hook_test;
