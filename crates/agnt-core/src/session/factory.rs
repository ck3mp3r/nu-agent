use std::str::FromStr;
use std::sync::Arc;

use rig_memory::Store;
use serde::{Deserialize, Serialize};

use super::SessionMeta;
use super::rig_dir_store;
use super::rig_sqlite_store::RigSqliteStore;
use super::rig_state::{self, SessionView};

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

/// Create a rig-memory store for one session.
///
/// `Sqlite` (the default) keeps every session in
/// `{cache}/{APP_DIR_NAME}/rig-sessions.db`; `Jsonl` keeps the session in
/// `{cache}/{APP_DIR_NAME}/sessions/{session_id}/`; `Memory` keeps it in an
/// in-memory SQLite database. `APP_DIR_NAME` is `nu-agent-dev` in debug builds
/// and `nu-agent` in release builds.
///
/// The SQLite database is separate from the session store's `sessions.db`: the
/// rig store refuses a database that holds the pre-rig-memory schema, so the
/// two must not share a file.
pub fn create_rig_store(
    store_type: StoreType,
    session_id: &str,
) -> Result<Arc<dyn Store>, RigStoreError> {
    let cache_dir = crate::utils::xdg::cache_dir().map_err(|e| {
        RigStoreError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to resolve XDG cache directory: {e}"),
        ))
    })?;
    create_rig_store_in(store_type, &cache_dir, session_id)
}

/// Create a rig-memory store for one session rooted at an explicit cache
/// directory.
///
/// `Memory` ignores `cache_dir` entirely — it never touches the filesystem.
pub fn create_rig_store_in(
    store_type: StoreType,
    cache_dir: &std::path::Path,
    session_id: &str,
) -> Result<Arc<dyn Store>, RigStoreError> {
    match store_type {
        StoreType::Sqlite => {
            let path = crate::utils::paths::app_dir(cache_dir).join("rig-sessions.db");
            let path = path.to_str().ok_or_else(|| {
                RigStoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Non-UTF-8 path for SQLite database",
                ))
            })?;
            Ok(Arc::new(RigSqliteStore::open(path, session_id)?))
        }
        StoreType::Jsonl => Ok(Arc::new(rig_dir_store::open_session_store(
            cache_dir, session_id,
        )?)),
        StoreType::Memory => Ok(Arc::new(RigSqliteStore::in_memory(session_id)?)),
    }
}

/// The ids of the sessions `store_type` holds under the cache directory.
///
/// `Memory` keeps nothing across calls, so it lists no session.
pub fn list_rig_sessions(store_type: StoreType) -> Result<Vec<String>, RigStoreError> {
    let cache_dir = crate::utils::xdg::cache_dir().map_err(|e| {
        RigStoreError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to resolve XDG cache directory: {e}"),
        ))
    })?;
    list_rig_sessions_in(store_type, &cache_dir)
}

/// The ids of the sessions `store_type` holds under `cache_dir`.
///
/// `Memory` keeps nothing across calls, so it lists no session.
pub fn list_rig_sessions_in(
    store_type: StoreType,
    cache_dir: &std::path::Path,
) -> Result<Vec<String>, RigStoreError> {
    match store_type {
        StoreType::Sqlite => {
            let path = crate::utils::paths::app_dir(cache_dir).join("rig-sessions.db");
            let path = path.to_str().ok_or_else(|| {
                RigStoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Non-UTF-8 path for SQLite database",
                ))
            })?;
            RigSqliteStore::session_ids(path)
        }
        StoreType::Jsonl => rig_dir_store::session_ids(cache_dir),
        StoreType::Memory => Ok(Vec::new()),
    }
}

/// How a session's rig-memory store is made: the backend, and the cache
/// directory it lives under.
#[derive(Debug, Clone)]
pub struct RigStoreConfig {
    store_type: StoreType,
    cache_dir: Option<std::path::PathBuf>,
}

impl RigStoreConfig {
    /// The config of `store_type`, resolving the cache directory when a store
    /// is opened.
    pub fn new(store_type: StoreType) -> Self {
        Self {
            store_type,
            cache_dir: None,
        }
    }

    /// The config of `store_type` rooted at `cache_dir`.
    ///
    /// This is the seam the tests use: production resolves the cache directory
    /// from the XDG environment, while a test roots the store in a tempdir so
    /// it never touches the user's cache.
    pub fn with_cache_dir(self, cache_dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            store_type: self.store_type,
            cache_dir: Some(cache_dir.into()),
        }
    }

    /// The config of an in-memory store: nothing touches the filesystem.
    pub fn in_memory() -> Self {
        Self::new(StoreType::Memory)
    }

    /// The store type this config opens.
    pub fn store_type(&self) -> StoreType {
        self.store_type
    }

    /// Opens the rig-memory store of `session_id`.
    ///
    /// # Errors
    ///
    /// As [`create_rig_store`] and [`create_rig_store_in`].
    pub fn open(&self, session_id: &str) -> Result<Arc<dyn Store>, RigStoreError> {
        match &self.cache_dir {
            Some(cache_dir) => create_rig_store_in(self.store_type, cache_dir, session_id),
            None => create_rig_store(self.store_type, session_id),
        }
    }

    /// The ids of the sessions this config's store holds.
    ///
    /// # Errors
    ///
    /// As [`list_rig_sessions`] and [`list_rig_sessions_in`].
    pub fn session_ids(&self) -> Result<Vec<String>, RigStoreError> {
        match &self.cache_dir {
            Some(cache_dir) => list_rig_sessions_in(self.store_type, cache_dir),
            None => list_rig_sessions(self.store_type),
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
