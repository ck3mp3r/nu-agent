//! App-owned conversation-memory error type.
//!
//! rig 0.45 deleted its `MemoryError` type along with the
//! `ConversationMemory` trait. This mirrors the deleted shape so the memory
//! stack keeps its existing error surface.

/// Boxed error source for memory backend failures.
pub type MemoryBackendError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Errors produced by a conversation-memory backend.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    /// The backing store failed to load, append, or clear messages.
    #[error("Memory backend error: {0}")]
    Backend(#[source] MemoryBackendError),

    /// A history-shaping filter or policy rejected the loaded history.
    #[error("Memory policy error: {0}")]
    Policy(String),

    /// An internal invariant was violated (e.g. a poisoned in-process lock).
    /// Distinct from [`MemoryError::Backend`], which is reserved for failures
    /// of the underlying conversation store.
    #[error("Memory internal error: {0}")]
    Internal(String),
}

impl MemoryError {
    /// Wrap an arbitrary error from a backend implementation.
    pub fn backend<E>(source: E) -> Self
    where
        E: Into<MemoryBackendError>,
    {
        Self::Backend(source.into())
    }
}
