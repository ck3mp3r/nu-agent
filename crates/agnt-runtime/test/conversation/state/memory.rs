//! Tests for `MemoryState` holding a `CachedMemory` directly.
//!
//! These tests pin the observable behavior through `inner_memory()` access to
//! the wrapped `CachedMemory`.

use crate::conversation::state::memory::MemoryState;
use crate::session::RigStoreConfig;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Build a `MemoryState` backed by an in-memory rig store.
fn make_state() -> MemoryState {
    MemoryState::new(RigStoreConfig::in_memory())
}

/// `MemoryState` exposes the wrapped `CachedMemory` via `inner_memory()`, which
/// opens the rig-memory store of a session.
#[tokio::test]
async fn memory_state_inner_is_cached_memory_with_rig_store() -> Result<()> {
    let state = make_state();

    let inner = state.inner_memory();
    let store = inner.rig_store("conv-1").await.map_err(|_| "should open")?;
    let conversation = rig_memory::load(&*store, "conv-1").map_err(|_| "should load")?;
    assert!(
        conversation.messages().is_empty(),
        "fresh conversation has no messages"
    );
    Ok(())
}
