//! Tests for `MemoryState` holding a `CachedMemory` directly.
//!
//! These tests pin the observable behavior through the public
//! `ConversationMemory::load` API and through `inner_memory()` access to the
//! wrapped `CachedMemory`.

use std::sync::Arc;

use rig::memory::ConversationMemory;

use crate::conversation::state::memory::MemoryState;
use crate::session::FsSessionStore;
use crate::types::Message;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Build a `MemoryState` backed by a tempdir store.
fn make_state() -> (tempfile::TempDir, MemoryState<FsSessionStore>) {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let state = MemoryState::new(store);
    (temp_dir, state)
}

/// `MemoryState` exposes the wrapped `CachedMemory` via `inner_memory()`, which
/// supports `load_all()`.
#[tokio::test]
async fn memory_state_inner_is_cached_memory_with_load_all() -> Result<()> {
    let (_temp_dir, state) = make_state();

    let inner = state.inner_memory();
    let entries = inner.load_all("conv-1").await.map_err(|_| "should load")?;
    assert!(entries.is_empty(), "fresh conversation has no entries");
    Ok(())
}

/// `load()` on a session returns all persisted messages unchanged.
#[tokio::test]
async fn load_returns_all_appended_messages() -> Result<()> {
    let (_temp_dir, state) = make_state();
    let memory = Arc::clone(state.memory());
    let conv = "conv-load";

    for i in 0..5 {
        memory
            .append(conv, vec![Message::user(format!("user-{i}"))])
            .await
            .map_err(|_| "append user should succeed")?;
        memory
            .append(conv, vec![Message::assistant(format!("assistant-{i}"))])
            .await
            .map_err(|_| "append assistant should succeed")?;
    }

    let loaded = memory.load(conv).await.map_err(|_| "load should succeed")?;

    assert_eq!(loaded.len(), 10, "all messages returned unchanged");
    Ok(())
}

/// `clear()` clears the in-memory cache; a subsequent `load` re-reads the
/// persisted messages from the store.
#[tokio::test]
async fn clear_delegates_to_inner_cached_memory() -> Result<()> {
    let (_temp_dir, mut state) = make_state();
    let memory = Arc::clone(state.memory());
    let conv = "conv-clear";

    memory
        .append(
            conv,
            vec![Message::user("user-0"), Message::assistant("assistant-0")],
        )
        .await
        .map_err(|_| "append should succeed")?;

    memory.load(conv).await.map_err(|_| "load should succeed")?;

    state.clear();
    assert!(state.last_total_tokens().is_none());

    let loaded = memory
        .load(conv)
        .await
        .map_err(|_| "load after clear should succeed")?;
    assert!(
        !loaded.is_empty(),
        "load after clear must re-read persisted messages from the store"
    );
    Ok(())
}

/// `last_total_tokens` is tracked independently of the memory backend.
#[test]
fn last_total_tokens_tracked_independently() {
    let (_temp_dir, mut state) = make_state();
    assert!(state.last_total_tokens().is_none());
    *state.last_total_tokens_mut() = Some(1234);
    assert_eq!(state.last_total_tokens(), Some(1234));
}
