use agnt_kernel::CompactionSource;

use crate::hook::TurnUsageTotals;
use crate::protocol::contracts::UiMessageSnapshot;
use crate::session::SessionInfo;

/// Pure session state lifecycle — synchronous, no I/O.
/// Every runtime implements this: production, test fakes, mocks.
pub trait SessionState {
    /// Clear the current session state.
    fn clear_session(&mut self) {}

    /// Start a new session.
    fn new_session(&mut self) {}
}

/// Async session persistence I/O — load and list sessions from a store.
/// Uses `impl Future + Send` following the same pattern as
/// `McpManagement::set_mcp_server_enabled`.
pub trait SessionPersistence {
    /// Returns the runtime's working directory for CWD-scoped session listing.
    fn cwd(&self) -> &std::path::Path {
        std::path::Path::new("/")
    }

    /// Returns the session ID the runtime is currently attached to, or `None`
    /// when no session is active.
    ///
    /// Used by the router to detect a same-session A2A attach and skip the
    /// transcript clear/hydrate cycle.
    fn current_session_id(&self) -> Option<&str> {
        None
    }

    /// The session's cumulative LLM usage: the sums across every turn the
    /// session has run.
    ///
    /// The cells reset when the runtime switches sessions, so the totals cover
    /// the current session only. A runtime that tracks no usage reports the
    /// empty sum.
    fn session_usage(&self) -> TurnUsageTotals {
        TurnUsageTotals::default()
    }

    /// Run a compaction of the current session for `source`.
    ///
    /// Loads the full history from the store-backed memory and runs the shared
    /// compaction logic, emitting `Started`/`SummaryChunk`/`Completed`/`Failed`
    /// events on the bus. A no-op for runtimes without a persistent session.
    fn run_compaction(
        &mut self,
        _source: CompactionSource,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send {
        async move { Ok(()) }
    }

    /// Undo the newest compaction of the current session.
    ///
    /// Restores the entries that compaction replaced from the session's own
    /// log, emitting `Uncompacted` on the record stream and the spine, or a
    /// warning notice when there is nothing to undo. A no-op for runtimes
    /// without a persistent session.
    fn uncompact(
        &mut self,
        _source: CompactionSource,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send {
        async move { Ok(()) }
    }

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

    /// Attach the session with `session_id`, creating it when it does not exist.
    ///
    /// Returns the transcript snapshots to hydrate when the session already
    /// existed, or an empty vector when a fresh session was created. The
    /// runtime's current session becomes `session_id` either way.
    fn attach_session(
        &mut self,
        _session_id: &str,
    ) -> impl std::future::Future<Output = Result<Vec<UiMessageSnapshot>, String>> + Send {
        async move { Err("Session attach not supported".to_string()) }
    }
}
