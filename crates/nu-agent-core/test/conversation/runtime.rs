//! Runtime test suite: preamble assembly, permissions, provider dispatch,
//! compaction policy, tool visibility, MCP state, memory, persona,
//! multi-agent state, accessors, and ModelHandle construction.
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
#[path = "runtime/tools.rs"]
mod tools;
#[path = "runtime/wiremock.rs"]
mod wiremock_tests;

// endregion: --- Modules

use crate::conversation::providers::ClientCacheKey;
use crate::types::ToolDefinition;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Build a `MemoryState<FsSessionStore>` (no compaction — `CachedMemory` used
/// directly) so tests never invoke a real LLM.
fn test_memory_state() -> super::super::state::memory::MemoryState<crate::session::FsSessionStore> {
    let temp_dir = tempfile::tempdir().unwrap();
    let store = std::sync::Arc::new(crate::session::FsSessionStore::new(
        temp_dir.path().to_path_buf(),
    ));
    super::super::state::memory::MemoryState::new(store)
}
