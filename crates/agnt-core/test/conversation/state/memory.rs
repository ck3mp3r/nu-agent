//! Tests for `MemoryState` holding a `CachedMemory` directly.
//!
//! These tests pin the observable behavior through `inner_memory()` access to
//! the wrapped `CachedMemory` and through the token tracking on `MemoryState`.

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

/// `clear()` resets the token counter and leaves the rig store untouched: the
/// store is append-only, so the next turn reads the same conversation.
#[tokio::test]
async fn clear_resets_the_token_counter_and_keeps_the_store() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = make_state();
    let conv = "conv-clear";
    let store = state
        .inner_memory()
        .rig_store(conv)
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let mut conversation = rig_memory::load(&*store, conv)?;
    conversation.extend([Message::user("user-0"), Message::assistant("assistant-0")]);
    rig_memory::save(&*store, conv, &mut conversation)?;
    *state.last_total_tokens_mut() = Some(1234);

    // -- Exec
    state.clear();

    // -- Check
    assert!(
        state.last_total_tokens().is_none(),
        "clear must reset the token counter"
    );
    let stored = rig_memory::load(&*store, conv)?;
    assert_eq!(
        stored.messages().len(),
        2,
        "clear must not touch the append-only store"
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
