//! rig-memory `DirStore` wiring: one directory per session.
//!
//! A session directory holds `logs/` (one JSON-lines log per agent), `files/`
//! (the session's named files, its blobs under `files/blobs/`), and the `name`
//! file that holds the display name. Sessions saved by the JSONL store that
//! nu-agent kept before rig-memory are refused, never migrated.

// region:    --- Modules

use std::io;
use std::path::{Path, PathBuf};

use rig_memory::{DirStore, Files, SessionError, check_name};

use super::factory::RigStoreError;

// endregion: --- Modules

// region:    --- Constants

/// The file of a session's store that holds its display name.
pub const NAME_FILE: &str = "name";

/// The folder of the sessions of one application directory.
const SESSIONS: &str = "sessions";

/// The extension of a session file of the JSONL store nu-agent kept before
/// rig-memory.
const LEGACY_EXTENSION: &str = "jsonl";

// endregion: --- Constants

// region:    --- Public Functions

/// The directory of `session_id` under `cache_dir`.
pub fn session_dir(cache_dir: &Path, session_id: &str) -> PathBuf {
    sessions_root(&crate::utils::paths::app_dir(cache_dir)).join(session_id)
}

/// The directory holding one directory per session, inside `store_dir`.
///
/// `store_dir` is the app's store directory: `{cache}/{APP_DIR_NAME}` when the
/// store is derived from the cache base, or the parent of the resolved store
/// path when `[paths]` or `NU_AGENT_CACHE_HOME` re-roots it.
pub fn sessions_root(store_dir: &Path) -> PathBuf {
    store_dir.join(SESSIONS)
}

/// Opens the `DirStore` of `session_id` under an explicit sessions directory.
///
/// # Errors
///
/// [`RigStoreError::Legacy`] when the session was saved by the JSONL store
/// nu-agent kept before rig-memory; it is left as it is.
pub fn open_session_store_at(
    sessions_dir: &Path,
    session_id: &str,
) -> Result<DirStore, RigStoreError> {
    check_name(session_id)?;
    let dir = sessions_dir.join(session_id);
    refuse_legacy(&dir, session_id)?;
    match DirStore::open(&dir) {
        Ok(store) => Ok(store),
        Err(SessionError::OldLayout) => Err(RigStoreError::Legacy),
        Err(error) => Err(RigStoreError::Session(error)),
    }
}

/// Writes the display name of the session into its store.
pub fn write_session_name(files: &(impl Files + ?Sized), name: &str) -> io::Result<()> {
    files.write_file(NAME_FILE, name.as_bytes())
}

/// The display name of the session in its store; `None` when it has none.
pub fn read_session_name(files: &(impl Files + ?Sized)) -> io::Result<Option<String>> {
    match files.read_file(NAME_FILE) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
    }
}

/// The ids of the sessions under an explicit sessions directory, in order.
///
/// # Errors
///
/// When the sessions directory cannot be read.
pub fn session_ids_at(sessions_dir: &Path) -> Result<Vec<String>, RigStoreError> {
    let entries = match std::fs::read_dir(sessions_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(RigStoreError::Io(error)),
    };
    let mut ids = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && let Some(name) = entry.file_name().to_str()
        {
            ids.push(name.to_owned());
        }
    }
    ids.sort();
    Ok(ids)
}

// endregion: --- Public Functions

// region:    --- Support

/// Refuses `dir` when the JSONL store nu-agent kept before rig-memory saved
/// `session_id` beside it.
fn refuse_legacy(dir: &Path, session_id: &str) -> Result<(), RigStoreError> {
    let legacy = dir
        .parent()
        .map(|sessions| sessions.join(format!("{session_id}.{LEGACY_EXTENSION}")));
    match legacy {
        Some(legacy) if legacy.exists() => Err(RigStoreError::Legacy),
        _ => Ok(()),
    }
}

// endregion: --- Support
