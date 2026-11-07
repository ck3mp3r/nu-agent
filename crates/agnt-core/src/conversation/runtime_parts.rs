//! The parts a composition root hands to the aggregate runtime's constructor.

// region:    --- Types

use std::sync::Arc;

use agnt_runtime::bus::Bus;
use agnt_runtime::tools::closure::EngineInterfaceLike;

use super::state::{MultiAgentState, PersonaState, ProviderState, ToolState};
use agnt_runtime::conversation::state::memory::MemoryState;

/// The seven parts [`AgentRuntime::compose`](super::runtime::AgentRuntime::compose)
/// takes, one named field each.
///
/// Completeness is a compile-time property: a composition site that omits a
/// part does not compile, and no part can be replaced once the aggregate is
/// composed. The derived state — the erased models, the MCP and permission
/// state, the hook context, the compaction config — is built inside `compose`
/// from its configuration argument, so it is not a part.
pub struct RuntimeParts {
    pub provider: ProviderState,
    pub tools: ToolState,
    pub session: MemoryState,
    pub persona: PersonaState,
    pub multi: MultiAgentState,
    pub engine: Arc<dyn EngineInterfaceLike>,
    pub bus: Bus,
}

// endregion: --- Types
