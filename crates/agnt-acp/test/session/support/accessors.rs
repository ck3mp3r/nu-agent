//! The session-tree accessors the session pins call.

use super::*;

// region:    --- Session accessors

impl AcpSession {
    /// The conversation runtime that drives this session.
    ///
    /// Test-tree home of the former `pub` accessor: the session-level pins
    /// park on, seed, and read this session's actual runtime lock, which no
    /// public manager method exposes. The `#[cfg(test)]` tree plus `pub(crate)`
    /// keep the method off the census and out of the lib build.
    pub(crate) fn runtime(&self) -> &Arc<AsyncMutex<AgentConversationRuntime>> {
        &self.runtime
    }
}

// endregion: --- Session accessors
