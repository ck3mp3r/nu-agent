use std::str::FromStr;
use std::sync::Arc;

use rig_memory::Store;
use serde::{Deserialize, Serialize};

use super::SessionMeta;
use super::rig_dir_store;
use super::rig_sqlite_store::RigSqliteStore;
use super::rig_state::{self, SessionView};
use crate::config::paths::STORE_FILE;

/// The type of session store backend to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StoreType {
    /// SQLite-backed session store.
    #[default]
    #[serde(rename = "sqlite")]
    Sqlite,
    /// JSONL file-backed session store.
    #[serde(rename = "jsonl")]
    Jsonl,
    /// In-memory session store (backed by SQLite `:memory:`).
    #[serde(rename = "memory")]
    Memory,
}

impl FromStr for StoreType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sqlite" => Ok(StoreType::Sqlite),
            "jsonl" => Ok(StoreType::Jsonl),
            "memory" => Ok(StoreType::Memory),
            other => Err(format!(
                "Unknown store type: '{other}'. Expected 'sqlite', 'jsonl', or 'memory'."
            )),
        }
    }
}

/// Errors that can occur when opening a rig-memory session store.
#[derive(Debug, thiserror::Error)]
pub enum RigStoreError {
    /// The session was saved by a store nu-agent kept before rig-memory.
    #[error("this session was made by another version of nu-agent; start a new session")]
    Legacy,
    /// An I/O error occurred.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A SQLite error occurred.
    #[error(transparent)]
    Sqlite(#[from] sqlx::Error),
    /// rig-memory refused the session directory.
    #[error(transparent)]
    Session(#[from] rig_memory::SessionError),
    /// The session could not be read.
    #[error(transparent)]
    Memory(#[from] super::MemoryError),
}

/// Create a rig-memory store for one session at an explicit store path.
///
/// `Sqlite` opens `store_path` exactly. `Jsonl` keeps its session directories
/// beside it, under `<store_path parent>/sessions/`. `Memory` ignores the path.
///
/// This is the production entry: the caller passes the store path `Paths`
/// resolved, so no site re-joins the file name.
///
/// The SQLite database is separate from the session store's `sessions.db`: the
/// rig store refuses a database that holds the pre-rig-memory schema, so the
/// two must not share a file.
pub fn create_rig_store_at(
    store_type: StoreType,
    store_path: &std::path::Path,
    session_id: &str,
) -> Result<Arc<dyn Store>, RigStoreError> {
    match store_type {
        StoreType::Sqlite => Ok(Arc::new(RigSqliteStore::open(
            utf8_path(store_path)?,
            session_id,
        )?)),
        StoreType::Jsonl => Ok(Arc::new(rig_dir_store::open_session_store_at(
            &rig_dir_store::sessions_root(store_dir(store_path)),
            session_id,
        )?)),
        StoreType::Memory => Ok(Arc::new(RigSqliteStore::in_memory(session_id)?)),
    }
}

/// Create a rig-memory store for one session rooted at an explicit cache
/// directory.
///
/// `Sqlite` (the default) keeps every session in
/// `{cache_dir}/{APP_DIR_NAME}/rig-sessions.db`; `Jsonl` keeps the session in
/// `{cache_dir}/{APP_DIR_NAME}/sessions/{session_id}/`; `Memory` keeps it in an
/// in-memory SQLite database. `APP_DIR_NAME` is `nu-agent-dev` in debug builds
/// and `nu-agent` in release builds.
pub fn create_rig_store_in(
    store_type: StoreType,
    cache_dir: &std::path::Path,
    session_id: &str,
) -> Result<Arc<dyn Store>, RigStoreError> {
    let store_path = crate::utils::paths::app_dir(cache_dir).join(STORE_FILE);
    create_rig_store_at(store_type, &store_path, session_id)
}

/// The ids of the sessions `store_type` holds at an explicit store path.
///
/// `Memory` keeps nothing across calls, so it lists no session.
pub fn list_rig_sessions_at(
    store_type: StoreType,
    store_path: &std::path::Path,
) -> Result<Vec<String>, RigStoreError> {
    match store_type {
        StoreType::Sqlite => RigSqliteStore::session_ids(utf8_path(store_path)?),
        StoreType::Jsonl => {
            rig_dir_store::session_ids_at(&rig_dir_store::sessions_root(store_dir(store_path)))
        }
        StoreType::Memory => Ok(Vec::new()),
    }
}

/// The ids of the sessions `store_type` holds under an explicit cache
/// directory.
///
/// `Memory` keeps nothing across calls, so it lists no session.
pub fn list_rig_sessions_in(
    store_type: StoreType,
    cache_dir: &std::path::Path,
) -> Result<Vec<String>, RigStoreError> {
    let store_path = crate::utils::paths::app_dir(cache_dir).join(STORE_FILE);
    list_rig_sessions_at(store_type, &store_path)
}

/// Create a rig-memory store for one session under the XDG cache base.
///
/// This is the last-resort form: a caller that holds a resolved
/// [`Paths`](crate::config::Paths) passes its store path to
/// [`create_rig_store_at`] instead, so the store honors `[paths]` and
/// `NU_AGENT_CACHE_HOME`. `Memory` resolves no path at all.
pub fn create_rig_store(
    store_type: StoreType,
    session_id: &str,
) -> Result<Arc<dyn Store>, RigStoreError> {
    // A memory store keeps nothing on disk, so the ambient cache directory is
    // never resolved for it: short-circuit before the path lookup.
    if store_type == StoreType::Memory {
        return Ok(Arc::new(RigSqliteStore::in_memory(session_id)?));
    }
    create_rig_store_at(store_type, &cache_store_path()?, session_id)
}

/// The ids of the sessions `store_type` holds under the XDG cache base.
///
/// `Memory` keeps nothing across calls, so it lists no session.
pub fn list_rig_sessions(store_type: StoreType) -> Result<Vec<String>, RigStoreError> {
    // As [`create_rig_store`]: a memory store lists nothing and needs no path.
    if store_type == StoreType::Memory {
        return Ok(Vec::new());
    }
    list_rig_sessions_at(store_type, &cache_store_path()?)
}

/// How a session's rig-memory store is made: the backend, and where it lives.
///
/// A config either carries the store path [`Paths`](crate::config::Paths)
/// resolved ([`Self::with_store_path`], what production does), or the cache
/// directory its tree derives from ([`Self::with_cache_dir`], the test seam).
/// With neither, the store derives from the XDG cache base.
#[derive(Debug, Clone)]
pub struct RigStoreConfig {
    store_type: StoreType,
    cache_dir: Option<std::path::PathBuf>,
    store_path: Option<std::path::PathBuf>,
}

impl RigStoreConfig {
    // --- Constructors ---

    /// The config of `store_type`, with neither a store path nor a cache dir.
    pub fn new(store_type: StoreType) -> Self {
        Self {
            store_type,
            cache_dir: None,
            store_path: None,
        }
    }

    /// The config of `store_type` opening exactly `store_path`.
    ///
    /// This is how production root the store: `Paths.store_path` is already
    /// resolved, so this config never re-derives it. `Sqlite` opens the file;
    /// `Jsonl` keeps its session directories in its parent's `sessions/`.
    pub fn with_store_path(self, store_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            store_type: self.store_type,
            cache_dir: None,
            store_path: Some(store_path.into()),
        }
    }

    /// The config of `store_type` rooted at `cache_dir`.
    ///
    /// This is the seam the tests use: a test roots the store in a tempdir so
    /// it never touches the user's cache.
    pub fn with_cache_dir(self, cache_dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            store_type: self.store_type,
            cache_dir: Some(cache_dir.into()),
            store_path: None,
        }
    }

    /// The config of an in-memory store: nothing touches the filesystem.
    pub fn in_memory() -> Self {
        Self::new(StoreType::Memory)
    }

    // --- Accessors ---

    /// The store type this config opens.
    pub fn store_type(&self) -> StoreType {
        self.store_type
    }

    /// Opens the rig-memory store of `session_id`.
    ///
    /// # Errors
    ///
    /// As [`create_rig_store_at`], [`create_rig_store_in`], and
    /// [`create_rig_store`].
    pub fn open(&self, session_id: &str) -> Result<Arc<dyn Store>, RigStoreError> {
        match (&self.store_path, &self.cache_dir) {
            (Some(store_path), _) => create_rig_store_at(self.store_type, store_path, session_id),
            (None, Some(cache_dir)) => create_rig_store_in(self.store_type, cache_dir, session_id),
            (None, None) => create_rig_store(self.store_type, session_id),
        }
    }

    /// The ids of the sessions this config's store holds.
    ///
    /// # Errors
    ///
    /// As [`list_rig_sessions_at`], [`list_rig_sessions_in`], and
    /// [`list_rig_sessions`].
    pub fn session_ids(&self) -> Result<Vec<String>, RigStoreError> {
        match (&self.store_path, &self.cache_dir) {
            (Some(store_path), _) => list_rig_sessions_at(self.store_type, store_path),
            (None, Some(cache_dir)) => list_rig_sessions_in(self.store_type, cache_dir),
            (None, None) => list_rig_sessions(self.store_type),
        }
    }

    /// Whether the store of `session_id` holds the session.
    ///
    /// # Errors
    ///
    /// As [`Self::open`].
    pub fn holds(&self, session_id: &str) -> Result<bool, RigStoreError> {
        let logs = self.open(session_id)?.logs()?;
        Ok(logs.iter().any(|log| log == session_id))
    }

    /// The listing of `session_id`: its `meta.json`, or the default when it has
    /// none.
    ///
    /// # Errors
    ///
    /// As [`Self::open`].
    pub fn listing(&self, session_id: &str) -> Result<SessionMeta, RigStoreError> {
        let store = self.open(session_id)?;
        Ok(SessionMeta::read(&*store).unwrap_or_default())
    }

    /// The whole session `session_id`: its listing, its main agent's model,
    /// title and conversation, and what reading it repaired.
    ///
    /// # Errors
    ///
    /// As [`Self::open`], or when the session cannot be read.
    pub fn view(&self, session_id: &str) -> Result<SessionView, RigStoreError> {
        let store = self.open(session_id)?;
        rig_state::read_view(&*store).map_err(RigStoreError::from)
    }

    /// Delete the store of `session_id`: its logs and its files.
    ///
    /// # Errors
    ///
    /// As [`Self::open`], or when the store cannot be deleted.
    pub fn destroy(&self, session_id: &str) -> Result<(), RigStoreError> {
        self.open(session_id)?.destroy()?;
        Ok(())
    }
}

/// The store path under the XDG cache base.
fn cache_store_path() -> Result<std::path::PathBuf, RigStoreError> {
    let cache_dir = crate::utils::xdg::cache_dir().map_err(|e| {
        RigStoreError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to resolve XDG cache directory: {e}"),
        ))
    })?;
    Ok(crate::utils::paths::app_dir(cache_dir).join(STORE_FILE))
}

/// The store directory a resolved store path belongs to.
fn store_dir(store_path: &std::path::Path) -> &std::path::Path {
    store_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
}

/// `path` as `&str`, refusing a non-UTF-8 path.
fn utf8_path(path: &std::path::Path) -> Result<&str, RigStoreError> {
    path.to_str().ok_or_else(|| {
        RigStoreError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Non-UTF-8 path for SQLite database",
        ))
    })
}
