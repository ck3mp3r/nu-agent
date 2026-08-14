use crate::protocol::contracts::UiMessageSnapshot;
use crate::session::SessionInfo;

/// Pure session state lifecycle — synchronous, no I/O.
/// Every runtime implements this: production, test fakes, mocks.
pub trait SessionState {
    /// Clear the current session state.
    fn clear_session(&mut self) {}

    /// Start a new session.
    fn new_session(&mut self) {}

    /// Seed `MemoryState.last_total_tokens` from a loaded session so that
    /// compaction can fire on the first turn after a session resume.
    ///
    /// Called by `run_hydrated_interactive_loop_with_external_prompts`
    /// immediately after the UI transcript hydration. A no-op for test runtimes
    /// that do not have a `MemoryState`.
    fn seed_last_total_tokens(&mut self, _tokens: Option<u64>) {}
}

/// Async session persistence I/O — load and list sessions from a store.
/// Uses `impl Future + Send` following the same pattern as
/// `Compaction::execute_compaction_trigger` and
/// `McpManagement::set_mcp_server_enabled`.
pub trait SessionPersistence {
    /// Load a session by ID and return its messages as UI snapshots.
    /// Returns an error string if the session doesn't exist or can't be loaded.
    fn load_session(
        &mut self,
        _session_id: &str,
    ) -> impl std::future::Future<Output = Result<Vec<UiMessageSnapshot>, String>> + Send {
        async move { Err("Session loading not supported".to_string()) }
    }

    /// List all available sessions.
    fn list_sessions(
        &self,
        _cwd: &std::path::Path,
    ) -> impl std::future::Future<Output = Result<Vec<SessionInfo>, String>> + Send {
        async move { Ok(Vec::new()) }
    }
}
