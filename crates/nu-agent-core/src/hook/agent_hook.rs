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

/// Returns `true` if a tool result string represents a failure injected by any
/// code path in the agent hook.
///
/// The following strings are failure indicators:
/// - `"Toolset error: "` — rig toolset execution errors
/// - `"Permission denied"` — permission denial from `on_tool_call`
/// - `"Doom loop detected: "` — doom loop guard in `on_tool_call`
/// - `"Tool '"` — invalid/unavailable tool skip from `on_invalid_tool_call`
/// - `"Sub-turn tool call limit reached"` — per-sub-turn cap from `on_tool_call`
pub(crate) fn is_tool_failure(result_text: &str) -> bool {
    result_text.starts_with("Toolset error: ")
        || result_text == "Permission denied"
        || result_text.starts_with("Doom loop detected: ")
        || result_text.starts_with("Tool '")
        || result_text.starts_with("Sub-turn tool call limit reached")
}

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
