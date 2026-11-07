//! Manager traits for the decomposed `AgentRuntime` components.
//!
//! Each trait captures the minimal interface that `AgentRuntime` method bodies
//! actually call — no phantom methods, no speculative surface area.
//!
//! All traits use static dispatch (generics with trait bounds) per AGENTS.md:
//! no `Box<dyn Trait>`, no `&dyn Trait`.

// region:    --- SessionManager

/// Manages conversation memory and session IDs.
///
/// `Sync` is required because `persist_turn_delta` is an async method taking
/// `&self`: its future holds `&TurnExecutor` across the bus `send().await`, and
/// `&TurnExecutor: Send` requires `TurnExecutor: Sync`. The executor holds
/// `&mut Self`, so that reduces to `Self: Sync`. `Send` alone is not enough:
/// `&mut T: Send` needs only `T: Send`, but `&mut T: Sync` needs `T: Sync`.
pub trait SessionManager: Sync {
    /// The concrete memory type backed by this manager.
    type Memory;

    /// The inner concrete memory backend wrapped by the compacting facade.
    type InnerMemory;

    /// Return a shared reference to the underlying memory.
    fn memory(&self) -> &Self::Memory;

    /// Return a mutable reference to the underlying memory.
    fn memory_mut(&mut self) -> &mut Self::Memory;

    /// Return a reference to the inner concrete memory backend (the `CachedMemory`
    /// held by the memory state). Used by callers that need the concrete
    /// `CachedMemory` API (store rewrites, marker writes, raw loads).
    fn inner_memory(&self) -> &Self::InnerMemory;

    /// The rig-memory store of `session_id`, made on first use and kept.
    fn rig_store(
        &self,
        session_id: &str,
    ) -> impl std::future::Future<
        Output = Result<std::sync::Arc<dyn rig_memory::Store>, crate::session::MemoryError>,
    > + Send;

    /// Clear all messages from conversation memory and reset token tracking.
    fn clear(&mut self);

    /// Return the last recorded total-token count (for compaction evaluation).
    fn last_total_tokens(&self) -> Option<u64>;

    /// Return a mutable reference to the last total token counter.
    fn last_total_tokens_mut(&mut self) -> &mut Option<u64>;
}

// endregion: --- SessionManager
