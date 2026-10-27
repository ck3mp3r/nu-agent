//! Runtime test suite: preamble assembly, permissions, provider dispatch,
//! compaction policy, tool visibility, MCP state, memory, persona,
//! multi-agent state, accessors, and DynModel<Completion> construction.
//!
//! The suite is split into topical sub-files. Shared imports, the `Result`
//! alias, and the `test_memory_state` helper live here; each sub-file pulls
//! them in with `use super::*;`.

use super::*;

// region:    --- Modules

#[path = "runtime/accessors.rs"]
mod accessors;
#[path = "runtime/compaction.rs"]
mod compaction;
#[path = "runtime/depth.rs"]
mod depth;
#[cfg(all(test, feature = "integration"))]
#[path = "runtime/depth_gated.rs"]
mod depth_gated;
#[path = "runtime/harness.rs"]
mod harness;
#[path = "runtime/lifecycle.rs"]
mod lifecycle;
#[path = "runtime/mcp.rs"]
mod mcp;
#[path = "runtime/memory.rs"]
mod memory;
#[path = "runtime/multi_agent.rs"]
mod multi_agent;
#[path = "runtime/permissions.rs"]
mod permissions;
#[path = "runtime/persona.rs"]
mod persona;
#[path = "runtime/preamble.rs"]
mod preamble;
#[path = "runtime/provider.rs"]
mod provider;
#[path = "runtime/session_usage.rs"]
mod session_usage;
#[cfg(all(test, feature = "integration"))]
#[path = "runtime/switch_compaction_threshold.rs"]
mod switch_compaction_threshold;
#[cfg(all(test, feature = "integration"))]
#[path = "runtime/switch_context_gate.rs"]
mod switch_context_gate;
#[cfg(all(test, feature = "integration"))]
#[path = "runtime/switch_events.rs"]
mod switch_events;
#[cfg(all(test, feature = "integration"))]
#[path = "runtime/switch_support.rs"]
mod switch_support;
#[path = "runtime/switching.rs"]
mod switching;
#[path = "runtime/tools.rs"]
mod tools;
#[path = "runtime/turn.rs"]
mod turn;
#[cfg(all(test, feature = "integration"))]
#[path = "runtime/wiremock.rs"]
mod wiremock_tests;

// endregion: --- Modules

use crate::conversation::providers::ClientCacheKey;
use crate::types::{ToolDefinition, ToolName};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Build a `MemoryState` over an in-memory rig store (no compaction —
/// `CachedMemory` used directly) so tests never invoke a real LLM.
fn test_memory_state() -> super::super::state::memory::MemoryState {
    super::super::state::memory::MemoryState::new(crate::session::RigStoreConfig::in_memory())
}
