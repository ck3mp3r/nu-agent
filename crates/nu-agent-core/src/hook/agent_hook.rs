//! AgentHook — transition shim.
//!
//! `AgentHook<P>` is now a type alias for [`HookChain<P>`].
//! All implementation lives in the concern modules and `chain.rs`.

use std::sync::{Arc, Mutex};

use crate::tools::mcp::circuit_breaker::McpCircuitBreaker;

pub use crate::hook::chain::HookChain;
pub use crate::hook::doom_loop::{DOOM_LOOP_THRESHOLD, DoomLoopState};
pub use crate::hook::permission_resolver::{AsyncPermissionResolver, PermissionDecision};

// Re-export rig hook action types used in tests via `super::*`
pub use rig::agent::{HookAction, ToolCallHookAction};

/// Returns `true` if a tool result string represents a failure injected by any
/// code path in the agent hook.
///
/// The following strings are failure indicators:
/// - `"Toolset error: "` — rig toolset execution errors
/// - `"Permission denied"` — permission denial from `on_tool_call`
/// - `"Doom loop detected: "` — doom loop guard in `on_tool_call`
/// - `"Tool '"` — invalid/unavailable tool skip from `on_invalid_tool_call`
/// - `"Tool call limit exceeded"` — per-sub-turn cap from `on_tool_call`
pub(crate) fn is_tool_failure(result_text: &str) -> bool {
    result_text.starts_with("Toolset error: ")
        || result_text == "Permission denied"
        || result_text.starts_with("Doom loop detected: ")
        || result_text.starts_with("Tool '")
        || result_text.starts_with("Tool call limit exceeded")
}

/// Session-scoped state shared across turns via the hook.
///
/// Bundles the `Arc<Mutex<…>>` state that outlives individual turns and
/// must accumulate across them (circuit breaker failures, doom loop
/// signatures).
#[derive(Clone)]
pub struct HookState {
    pub circuit_breaker: Arc<Mutex<McpCircuitBreaker>>,
    pub doom_state: Arc<Mutex<DoomLoopState>>,
}

/// Type alias kept for transition compatibility.
///
/// All callsites should be updated to use [`HookChain`] directly.
pub type AgentHook<P> = HookChain<P>;

#[cfg(test)]
#[path = "agent_hook_test.rs"]
mod agent_hook_test;
