// region:    --- Modules

use std::io;
use std::path::Path;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};

use super::actor::{Job, RigSqliteStore, SCHEMA_FILES, SCHEMA_LOGS, spawn_worker};
use crate::session::factory::RigStoreError;

// endregion: --- Modules

// region:    --- Constants

/// The database path that keeps the store in memory.
const IN_MEMORY: &str = ":memory:";

/// How long a write waits for another writer's lock.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

// endregion: --- Constants

// region:    --- Constructors

impl RigSqliteStore {
    /// Opens the store of `session_id` at `path`, creating the database and
    /// its tables when they are missing.
    ///
    /// # Errors
    ///
    /// [`RigStoreError::Io`] or [`RigStoreError::Sqlite`] when the database or
    /// its tables cannot be opened; a database of an older nu-agent opens and
    /// starts empty.
    pub fn open(path: &str, session_id: impl Into<String>) -> Result<Self, RigStoreError> {
        let (jobs, worker) = spawn_worker(path)?;
        Ok(Self {
            session_id: session_id.into(),
            jobs: Mutex::new(Some(jobs)),
            worker: Mutex::new(Some(worker)),
        })
    }

    /// Opens the store of `session_id` in memory: nothing touches the
    /// filesystem.
    ///
    /// # Errors
    ///
    /// When the in-memory database cannot be opened.
    pub fn in_memory(session_id: impl Into<String>) -> Result<Self, RigStoreError> {
        Self::open(IN_MEMORY, session_id)
    }

    /// The ids of the sessions the database at `path` holds, in order.
    ///
    /// # Errors
    ///
    /// [`RigStoreError::Io`] or [`RigStoreError::Sqlite`] when the database
    /// cannot be opened; a database of an older nu-agent lists no session.
    pub fn session_ids(path: &str) -> Result<Vec<String>, RigStoreError> {
        let (jobs, worker) = spawn_worker(path)?;
        let store = Self {
            session_id: String::new(),
            jobs: Mutex::new(Some(jobs)),
            worker: Mutex::new(Some(worker)),
        };
        store
            .submit_names(Job::SessionIds)
            .map_err(|error| RigStoreError::Io(io::Error::other(error)))
    }
}

// endregion: --- Constructors

// region:    --- Support

/// Opens the pool at `path` and creates the tables.
pub(super) async fn open_pool(path: &str) -> Result<SqlitePool, RigStoreError> {
    if path != IN_MEMORY
        && let Some(parent) = Path::new(path).parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent).await?;
    }
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .connect_with(connect_options(path)?)
        .await?;
    sqlx::query(SCHEMA_LOGS).execute(&pool).await?;
    sqlx::query(SCHEMA_FILES).execute(&pool).await?;
    Ok(pool)
}

fn connect_options(path: &str) -> Result<SqliteConnectOptions, RigStoreError> {
    let url = if path == IN_MEMORY {
        "sqlite::memory:".to_owned()
    } else {
        format!("sqlite://{path}")
    };
    Ok(SqliteConnectOptions::from_str(&url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(BUSY_TIMEOUT))
}

// endregion: --- Support
