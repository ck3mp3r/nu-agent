/// Runtime capability for session lifecycle management.
pub trait HasSessionManagement {
    fn clear_session(&mut self);

    fn new_session(&mut self);

    /// Seed `MemoryState.last_total_tokens` from a loaded session so that
    /// compaction can fire on the first turn after a session resume.
    ///
    /// Called by `run_hydrated_interactive_loop` immediately after the UI
    /// transcript hydration. A no-op for test runtimes that do not have a
    /// `MemoryState`.
    fn seed_last_total_tokens(&mut self, tokens: Option<u64>);
}
