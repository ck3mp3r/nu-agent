//! rig-memory's synchronous contracts, served by the engine.
//!
//! rig keeps a session as opaque bytes: one log per agent, plus the files the
//! session names. This module is the one place those contracts meet the
//! database: rig's `logs`/`files` tables keep their shape, every call becomes
//! one work unit on the engine, and a blob write lands with the `sessions` row
//! it implies in one transaction.
//!
//! rig's [`Store`](rig_memory::Store) is synchronous while the engine is not,
//! so each call submits its unit and waits for the answer. That wait works in
//! an asynchronous context as well as on a plain thread: rig's callers run
//! inside the host's async paths.

// region:    --- Modules

mod files;
mod store;
mod work;

use std::io;
use std::sync::Arc;

use crate::engine::Work;
use crate::error::{Result, StorageError};
use crate::storage::Storage;

use work::SessionIds;

// endregion: --- Modules

// region:    --- Types

/// One session's rig-memory [`Store`](rig_memory::Store), over the engine.
pub struct RigAdapter {
    pub(super) storage: Arc<Storage>,
    pub(super) session_id: String,
}

// endregion: --- Types

// region:    --- Constructors

impl RigAdapter {
    /// The store of `session_id` over the database `storage` serves.
    pub fn new(storage: Arc<Storage>, session_id: impl Into<String>) -> Self {
        Self {
            storage,
            session_id: session_id.into(),
        }
    }
}

// endregion: --- Constructors

// region:    --- Accessors

impl RigAdapter {
    /// The ids of the sessions the database of `storage` holds, in order.
    ///
    /// Every persisting write records its session, so the listing reads the
    /// `sessions` table instead of folding logs.
    ///
    /// # Errors
    ///
    /// [`StorageError`] when the engine refuses the unit.
    pub fn session_ids(storage: &Storage) -> Result<Vec<String>> {
        storage
            .call_blocking(SessionIds)
            .and_then(|ids| ids.map_err(StorageError::Sqlite))
    }
}

// endregion: --- Accessors

// region:    --- Support

impl RigAdapter {
    /// Runs `work` on the engine, answering the value its unit produced.
    pub(super) fn call<W: Work>(&self, work: W) -> io::Result<W::Output> {
        self.storage.call_blocking(work).map_err(io_error)
    }
}

/// `error` as an [`io::Error`], carrying its cause.
pub(super) fn io_error(error: StorageError) -> io::Error {
    io::Error::other(error)
}

/// SQLite's refusal as an [`io::Error`], carrying its cause.
pub(super) fn sql_error(error: sqlx::Error) -> io::Error {
    io::Error::other(error)
}

/// The error of a row that is not there.
pub(super) fn not_found() -> io::Error {
    io::ErrorKind::NotFound.into()
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/rig_adapter.rs"]
mod rig_adapter_test;

// endregion: --- Tests
