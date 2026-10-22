//! Tests for `MemoryState` holding a `CachedMemory` directly.
//!
//! These tests pin the observable behavior through the public
//! `CachedMemory::load` API and through `inner_memory()` access to the
//! wrapped `CachedMemory`.

use std::sync::Arc;

use crate::conversation::state::memory::MemoryState;
use crate::types::Message;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Build a `MemoryState` backed by an in-memory rig store.
fn make_state() -> MemoryState {
    MemoryState::new(crate::session::RigStoreConfig::in_memory())
}

/// `MemoryState` exposes the wrapped `CachedMemory` via `inner_memory()`, which
/// opens the rig-memory store of a session.
#[tokio::test]
async fn memory_state_inner_is_cached_memory_with_rig_store() -> Result<()> {
    let state = make_state();

    let inner = state.inner_memory();
    let store = inner.rig_store("conv-1").map_err(|_| "should open")?;
    let conversation = rig_memory::load(&*store, "conv-1").map_err(|_| "should load")?;
    assert!(
        conversation.messages().is_empty(),
        "fresh conversation has no messages"
    );
    Ok(())
}

/// `load()` on a session returns all persisted messages unchanged.
#[tokio::test]
async fn load_returns_all_appended_messages() -> Result<()> {
    let state = make_state();
    let memory = Arc::clone(state.memory());
    let conv = "conv-load";

    let mut seeded: Vec<Message> = Vec::new();
    for i in 0..5 {
        seeded.push(Message::user(format!("user-{i}")));
        seeded.push(Message::assistant(format!("assistant-{i}")));
    }
    memory.reset_context(conv, seeded);

    let loaded = memory.load(conv).await.map_err(|_| "load should succeed")?;

    assert_eq!(loaded.len(), 10, "all messages returned unchanged");
    Ok(())
}

/// `clear()` clears the in-memory cache; a subsequent `load` re-reads the
/// conversation from the rig store.
#[tokio::test]
async fn clear_delegates_to_inner_cached_memory() -> Result<()> {
    let mut state = make_state();
    let memory = Arc::clone(state.memory());
    let conv = "conv-clear";

    let store = memory.rig_store(conv).map_err(|_| "should open")?;
    let mut conversation = rig_memory::load(&*store, conv).map_err(|_| "should load")?;
    conversation.extend([Message::user("user-0"), Message::assistant("assistant-0")]);
    rig_memory::save(&*store, conv, &mut conversation).map_err(|_| "should save")?;

    let loaded = memory.load(conv).await.map_err(|_| "load should succeed")?;
    assert_eq!(loaded.len(), 2, "the rig store holds the seeded exchange");

    state.clear();
    assert!(state.last_total_tokens().is_none());

    let loaded = memory
        .load(conv)
        .await
        .map_err(|_| "load after clear should succeed")?;
    assert_eq!(
        loaded.len(),
        2,
        "load after clear must re-read the conversation from the rig store"
    );
    Ok(())
}

/// `last_total_tokens` is tracked independently of the memory backend.
#[test]
fn last_total_tokens_tracked_independently() {
    let mut state = make_state();
    assert!(state.last_total_tokens().is_none());
    *state.last_total_tokens_mut() = Some(1234);
    assert_eq!(state.last_total_tokens(), Some(1234));
}
