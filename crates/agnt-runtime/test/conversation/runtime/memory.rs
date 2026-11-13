use super::*;

// ========================================================================
// Memory and conversation store tests
// ========================================================================

#[test]
fn runtime_struct_has_memory_field() {
    // Compile-time check that the field exists with correct type: the runtime
    // memory is an `Arc<CachedMemory>`.
    use crate::conversation::state::memory::MemoryOf;

    // Compile-time check that the field exists with correct type
    fn _assert_field_exists(_: &MemoryOf) {}

    let _type_check: fn(&AgentConversationRuntime) = |r| {
        _assert_field_exists(r.session.memory());
    };
}

// ========================================================================
// Memory hydration guard tests (now: JournalConversationMemory)
// ========================================================================

#[test]
fn runtime_struct_has_memory_state_field() {
    // Compile-time check that the memory_state field exposes `memory()`.
    let _type_check: fn(&AgentConversationRuntime) = |r| {
        let _: &crate::conversation::state::memory::MemoryOf = r.session.memory();
    };
}

#[test]
fn runtime_struct_has_session_store() {
    // MemoryState wraps a rig-store-backed CachedMemory: `inner_memory()`
    // answers the concrete backend the store rewrites and markers reach.
    let ms = crate::conversation::state::memory::MemoryState::new(
        crate::session::RigStoreConfig::in_memory(),
    );
    let _inner: &crate::session::CachedMemory = ms.inner_memory();
}
