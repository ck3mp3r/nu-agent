use std::str::FromStr;
use std::sync::Arc;

use agnt_storage::{RigAdapter, Storage};
use rig_memory::Store;
use serde::{Deserialize, Serialize};

use super::SessionMeta;
use super::blocking::{PoolError, on_pool};
use super::rig_state::{self, SessionView};
use crate::config::paths::STORE_FILE;

/// The type of session store backend to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StoreType {
    /// SQLite-backed session store: one database file holds every session.
    #[default]
    #[serde(rename = "sqlite")]
    Sqlite,
    /// In-memory session store (backed by SQLite `:memory:`).
    #[serde(rename = "memory")]
    Memory,
}

impl FromStr for StoreType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sqlite" => Ok(StoreType::Sqlite),
            "memory" => Ok(StoreType::Memory),
            other => Err(format!(
                "Unknown store type: '{other}'. Expected 'sqlite' or 'memory'."
            )),
        }
    }
}

/// Errors that can occur when opening a rig-memory session store.
#[derive(Debug, thiserror::Error)]
pub enum RigStoreError {
    /// An I/O error occurred.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The storage engine refused the database or a unit on it.
    #[error(transparent)]
    Storage(#[from] agnt_storage::StorageError),
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

// region:    --- Froms

impl From<PoolError> for RigStoreError {
    /// Fold a pool failure into the family's I/O error, which keeps it as the
    /// cause and prints it in the message.
    fn from(error: PoolError) -> Self {
        Self::Io(std::io::Error::other(error))
    }
}

// endregion: --- Froms

/// Create a rig-memory store for one session at an explicit store path.
///
/// `Sqlite` opens `store_path` exactly. `Memory` ignores the path.
///
/// This is the production entry: the caller passes the store path `Paths`
/// resolved, so no site re-joins the file name.
///
/// `Sqlite` hands out one handle per session over that one database file: the
/// blobs are rig's, the engine under them is `agnt-storage`'s.
pub fn create_rig_store_at(
    store_type: StoreType,
    store_path: &std::path::Path,
    session_id: &str,
) -> Result<Arc<dyn Store>, RigStoreError> {
    match store_type {
        StoreType::Sqlite => {
            let storage = Arc::new(Storage::open(store_path)?);
            Ok(Arc::new(RigAdapter::new(storage, session_id)))
        }
        StoreType::Memory => {
            let storage = Arc::new(Storage::in_memory()?);
            Ok(Arc::new(RigAdapter::new(storage, session_id)))
        }
    }
}

/// Create a rig-memory store for one session rooted at an explicit cache
/// directory.
///
/// `Sqlite` (the default) keeps every session in
/// `{cache_dir}/{APP_DIR_NAME}/sessions.db`; `Memory` keeps it in an in-memory
/// SQLite database. `APP_DIR_NAME` is `nu-agent-dev` in debug builds and
/// `nu-agent` in release builds.
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
        StoreType::Sqlite => {
            let storage = Storage::open(store_path)?;
            Ok(RigAdapter::session_ids(&storage)?)
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
        let storage = Arc::new(Storage::in_memory()?);
        return Ok(Arc::new(RigAdapter::new(storage, session_id)));
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
    /// `Memory` ignores the path and keeps the database in memory.
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

    // --- Facade ---
    //
    // Every store interaction of this family crosses the blocking pool: rig's
    // store contracts are synchronous and its callers are async, so the work
    // runs on the pool and never parks a runtime thread.

    /// Opens the rig-memory store of `session_id`.
    ///
    /// # Errors
    ///
    /// As [`create_rig_store_at`], [`create_rig_store_in`], and
    /// [`create_rig_store`].
    pub async fn open(&self, session_id: &str) -> Result<Arc<dyn Store>, RigStoreError> {
        let config = self.clone();
        let session = session_id.to_owned();
        on_pool(move || config.open_blocking(&session))
            .await
            .map_err(RigStoreError::from)?
    }

    /// The ids of the sessions this config's store holds.
    ///
    /// # Errors
    ///
    /// As [`list_rig_sessions_at`], [`list_rig_sessions_in`], and
    /// [`list_rig_sessions`].
    pub async fn session_ids(&self) -> Result<Vec<String>, RigStoreError> {
        let config = self.clone();
        on_pool(move || config.session_ids_blocking())
            .await
            .map_err(RigStoreError::from)?
    }

    /// Whether the store of `session_id` holds the session.
    ///
    /// # Errors
    ///
    /// As [`Self::open`].
    pub async fn holds(&self, session_id: &str) -> Result<bool, RigStoreError> {
        let store = self.open(session_id).await?;
        let session = session_id.to_owned();
        let logs = on_pool(move || store.logs())
            .await
            .map_err(RigStoreError::from)?
            .map_err(RigStoreError::Io)?;
        Ok(logs.iter().any(|log| log == &session))
    }

    /// The listing of `session_id`: its `meta.json`, or the default when it has
    /// none.
    ///
    /// # Errors
    ///
    /// As [`Self::open`].
    pub async fn listing(&self, session_id: &str) -> Result<SessionMeta, RigStoreError> {
        let store = self.open(session_id).await?;
        on_pool(move || {
            Ok::<SessionMeta, RigStoreError>(SessionMeta::read(&*store).unwrap_or_default())
        })
        .await
        .map_err(RigStoreError::from)?
    }

    /// The whole session `session_id`: its listing, its main agent's model,
    /// title and conversation, and what reading it repaired.
    ///
    /// # Errors
    ///
    /// As [`Self::open`], or when the session cannot be read.
    pub async fn view(&self, session_id: &str) -> Result<SessionView, RigStoreError> {
        let store = self.open(session_id).await?;
        rig_state::read_view(store)
            .await
            .map_err(RigStoreError::from)
    }

    /// Delete the store of `session_id`: its logs and its files.
    ///
    /// # Errors
    ///
    /// As [`Self::open`], or when the store cannot be deleted.
    pub async fn destroy(&self, session_id: &str) -> Result<(), RigStoreError> {
        let store = self.open(session_id).await?;
        on_pool(move || store.destroy())
            .await
            .map_err(RigStoreError::from)?
            .map_err(RigStoreError::Io)
    }

    // --- Blocking internals ---

    /// The body of [`Self::open`], run on the blocking pool.
    fn open_blocking(&self, session_id: &str) -> Result<Arc<dyn Store>, RigStoreError> {
        match (&self.store_path, &self.cache_dir) {
            (Some(store_path), _) => create_rig_store_at(self.store_type, store_path, session_id),
            (None, Some(cache_dir)) => create_rig_store_in(self.store_type, cache_dir, session_id),
            (None, None) => create_rig_store(self.store_type, session_id),
        }
    }

    /// The body of [`Self::session_ids`], run on the blocking pool.
    fn session_ids_blocking(&self) -> Result<Vec<String>, RigStoreError> {
        match (&self.store_path, &self.cache_dir) {
            (Some(store_path), _) => list_rig_sessions_at(self.store_type, store_path),
            (None, Some(cache_dir)) => list_rig_sessions_in(self.store_type, cache_dir),
            (None, None) => list_rig_sessions(self.store_type),
        }
    }

    /// The engine of this config's database, the one the fact services write
    /// through.
    ///
    /// `Sqlite` opens the file the rig store opens; `Memory` opens an in-memory
    /// database of its own, which keeps nothing beyond the call.
    ///
    /// # Errors
    ///
    /// As [`create_rig_store_at`].
    pub(crate) fn facts_blocking(&self) -> Result<Arc<Storage>, RigStoreError> {
        match self.store_type {
            StoreType::Memory => Ok(Arc::new(Storage::in_memory()?)),
            StoreType::Sqlite => {
                let store_path = match (&self.store_path, &self.cache_dir) {
                    (Some(store_path), _) => store_path.clone(),
                    (None, Some(cache_dir)) => {
                        crate::utils::paths::app_dir(cache_dir).join(STORE_FILE)
                    }
                    (None, None) => cache_store_path()?,
                };
                Ok(Arc::new(Storage::open(store_path)?))
            }
        }
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
