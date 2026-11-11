//! The error every storage operation reports.

// region:    --- Modules

use std::io;

use thiserror::Error;

// endregion: --- Modules

// region:    --- Types

pub type Result<T> = core::result::Result<T, StorageError>;

/// What went wrong opening a database or running a unit on it.
#[derive(Debug, Error)]
pub enum StorageError {
    /// A failure this crate names itself.
    #[error("{0}")]
    Custom(String),

    /// The handle has no engine left to submit to.
    #[error("the storage engine is closed")]
    EngineClosed,

    /// The engine stopped before it answered the unit.
    #[error("the storage engine stopped before it answered")]
    EngineStopped,

    // -- Externals
    /// The database file or its directories could not be reached.
    #[error(transparent)]
    Io(#[from] io::Error),

    /// The framework's migrations refused the database or failed on it.
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),

    /// SQLite refused the statement.
    #[error(transparent)]
    Sqlite(#[from] sqlx::Error),
}

// endregion: --- Types
