/// Runtime capability for session lifecycle management.
pub trait HasSessionManagement {
    /// Clear the current session state.
    fn clear_session(&mut self) {}

    /// Start a new session.
    fn new_session(&mut self) {}

    /// Seed `MemoryState.last_total_tokens` from a loaded session so that
    /// compaction can fire on the first turn after a session resume.
    ///
    /// Called by `run_hydrated_interactive_loop` immediately after the UI
    /// transcript hydration. A no-op for test runtimes that do not have a
    /// `MemoryState`.
    fn seed_last_total_tokens(&mut self, _tokens: Option<u64>) {}
}
