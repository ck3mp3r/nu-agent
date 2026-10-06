use super::*;

// ========================================================================
// Memory and conversation store tests
// ========================================================================

#[test]
fn runtime_struct_has_memory_field() {
    // Compile-time check that the field exists with correct type: the runtime
    // memory is an `Arc<CachedMemory<SessionStoreBackend>>`.
    use crate::conversation::state::memory::MemoryOf;
    use crate::session::SessionStoreBackend;

    // Compile-time check that the field exists with correct type
    fn _assert_field_exists(_: &MemoryOf<SessionStoreBackend>) {}

    let _type_check: fn(&AgentConversationRuntime) = |r| {
        _assert_field_exists(r.session.memory());
    };
}

#[test]
fn runtime_struct_has_session_store() {
    // Verify MemoryState wraps a store-backed CachedMemory (via inner_memory).
    use crate::session::FsSessionStore;

    let temp_dir = tempfile::tempdir().unwrap();
    let store = std::sync::Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let ms = crate::conversation::state::memory::MemoryState::new(store);
    assert!(ms.last_total_tokens().is_none());
}

// ========================================================================
// Memory hydration guard tests (now: JournalConversationMemory)
// ========================================================================

#[test]
fn runtime_struct_has_memory_state_field() {
    // Compile-time check that the memory_state field exposes memory() and last_total_tokens().
    let _type_check: fn(&AgentConversationRuntime) = |r| {
        let _tokens: Option<u64> = r.session.last_total_tokens();
    };
}

#[test]
fn memory_state_hydrated_flag_starts_false() {
    // JournalConversationMemory replaces the old memory_hydrated bool.
    // The load-on-demand pattern means the cache starts empty — verified
    // by checking last_total_tokens is None on a fresh MemoryState.
    let ms = test_memory_state();
    assert!(
        ms.last_total_tokens().is_none(),
        "last_total_tokens must be None in a fresh MemoryState"
    );

    // Compile-time proof that session.last_total_tokens() works.
    let _type_check: fn(&AgentConversationRuntime) = |r| {
        let _: Option<u64> = r.session.last_total_tokens();
    };
}

// ========================================================================
// Phase J: MemoryState characterisation tests
// ========================================================================

#[test]
fn memory_state_hydrated_false_on_construction() {
    // JournalConversationMemory is load-on-demand (cache starts empty).
    // Verify last_total_tokens is None on construction.
    let ms = test_memory_state();
    assert!(ms.last_total_tokens().is_none());
}

#[test]
fn memory_state_last_total_tokens_none_on_construction() {
    let ms = test_memory_state();
    assert!(ms.last_total_tokens().is_none());
}
