//! App-owned conversation-memory error type.
//!
//! rig 0.45 deleted its `MemoryError` type along with the
//! `ConversationMemory` trait. This mirrors the deleted shape so the memory
//! stack keeps its existing error surface.

use super::factory::RigStoreError;

/// Why a conversation-memory backend failed.
///
/// Flat on purpose: [`RigStoreError`] holds a [`MemoryError`] by value, so a
/// variant carrying it here would make this type its own size
/// (`MemoryBackendError` → `RigStoreError` → `MemoryError` →
/// `MemoryBackendError`). A `RigStoreError` converts at the [`MemoryError`]
/// level instead, in `impl From<RigStoreError> for MemoryError`.
#[derive(Debug, thiserror::Error)]
pub enum MemoryBackendError {
    /// The store failed to read or write a file.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// The storage engine failed.
    #[error(transparent)]
    Storage(#[from] agnt_storage::StorageError),

    /// The blocking pool never ran the work, or the work panicked.
    #[error(transparent)]
    Pool(#[from] super::blocking::PoolError),

    /// The SQLite store failed.
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),

    /// rig-memory refused the session.
    #[error(transparent)]
    Rig(#[from] rig_memory::SessionError),

    /// A backend failure named by a message.
    #[error("{0}")]
    Message(String),
}

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

    /// The refusal text when the store answered the database as newer than this
    /// build migrates to, or as the legacy sqlx-migration layout.
    ///
    /// Both refusals say the same thing: the store cannot be written. A turn
    /// that ran anyway would persist nothing and report success, so the turn
    /// path fails on this text instead of degrading. Every other failure keeps
    /// the caller's warn-and-degrade.
    pub fn refusal_text(&self) -> Option<String> {
        match self {
            Self::Backend(MemoryBackendError::Storage(
                agnt_storage::StorageError::SchemaTooNew { .. }
                | agnt_storage::StorageError::LegacyStore { .. },
            )) => Some(self.to_string()),
            _ => None,
        }
    }
}

// region:    --- Froms

impl From<String> for MemoryBackendError {
    /// A message is not an `Error`, so it carries no source of its own.
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

impl From<RigStoreError> for MemoryError {
    /// Move the store failure's own payload into the matching
    /// [`MemoryBackendError`] variant, at the [`MemoryError`] level.
    fn from(error: RigStoreError) -> Self {
        match error {
            RigStoreError::Io(error) => Self::backend(error),
            RigStoreError::Storage(error) => Self::backend(error),
            RigStoreError::Sqlite(error) => Self::backend(error),
            RigStoreError::Session(error) => Self::backend(error),
            // The cycle break: the payload already is a `MemoryError`.
            RigStoreError::Memory(error) => error,
        }
    }
}

// endregion: --- Froms
