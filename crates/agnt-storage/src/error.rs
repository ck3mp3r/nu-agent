//! The error every storage operation reports.

// region:    --- Modules

use std::io;
use std::path::PathBuf;

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

    /// The database was written by a newer build than this one.
    #[error(
        "the database schema is version {stored}, newer than the {target} this build migrates to"
    )]
    SchemaTooNew { stored: u32, target: u32 },

    /// The database is the legacy sqlx-migration store, which this build
    /// refuses: its tables are not the engine's, and its `sessions` table
    /// collides with the first migration.
    #[error(
        "the database at {path} is the legacy sqlx-migration store (stored schema version {stored}, this build migrates to {target}); it holds tables this build did not create, and the file was left untouched"
    )]
    LegacyStore {
        stored: u32,
        target: u32,
        path: PathBuf,
    },

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

    /// SQLite refused the statement.
    #[error(transparent)]
    Sqlite(#[from] sqlx::Error),
}

// endregion: --- Types
