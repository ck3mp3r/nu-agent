//! The rig-memory `Files`/`Store` contract, served by the engine.
//!
//! The suite is split into topical sub-files. Shared imports and helpers live
//! here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[cfg(all(test, feature = "integration"))]
#[path = "rig_adapter/conformance.rs"]
mod conformance;

#[path = "rig_adapter/context.rs"]
mod context;

#[path = "rig_adapter/files.rs"]
mod files;

#[path = "rig_adapter/logs.rs"]
mod logs;

#[path = "rig_adapter/sessions.rs"]
mod sessions;

#[path = "rig_adapter/transcript.rs"]
mod transcript;

// endregion: --- Modules

use std::path::Path;
use std::sync::Arc;

use rig_memory::{Files, LogWriteError, Store};
use tempfile::TempDir;

use crate::{RigAdapter, Storage};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// An adapter over an in-memory database for `session_id`.
fn memory_store(session_id: &str) -> Result<RigAdapter> {
    let (_, store) = memory_database(session_id)?;
    Ok(store)
}

/// An adapter over an in-memory database for `session_id`, with the database
/// it writes to.
fn memory_database(session_id: &str) -> Result<(Arc<Storage>, RigAdapter)> {
    let storage = Arc::new(Storage::in_memory()?);
    let store = RigAdapter::new(storage.clone(), session_id);
    Ok((storage, store))
}

/// An adapter over the file database at `path` for `session_id`.
fn file_store(path: &Path, session_id: &str) -> Result<RigAdapter> {
    let storage = Arc::new(Storage::open(path)?);
    Ok(RigAdapter::new(storage, session_id))
}

/// The ids the database of `storage` lists, in order.
fn session_ids(storage: &Storage) -> Result<Vec<String>> {
    Ok(RigAdapter::session_ids(storage)?)
}
