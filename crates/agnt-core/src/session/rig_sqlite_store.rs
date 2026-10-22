//! A rig-memory [`Store`] over SQLite.
//!
//! One database holds every session: the `logs` and `files` tables are opaque
//! BLOB stores keyed by session id and name. [`Store`] is synchronous while
//! sqlx is asynchronous, so every call runs on a dedicated worker thread that
//! owns the connection pool and its runtime.

// region:    --- Modules

use std::any::Any;
use std::future::Future;
use std::io;
use std::path::Path;
use std::str::FromStr;
use std::sync::mpsc;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use rig_memory::{Files, LogWriteError, Store, check_name};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};

use super::factory::RigStoreError;

// endregion: --- Modules

// region:    --- Constants

/// The database path that keeps the store in memory.
const IN_MEMORY: &str = ":memory:";

/// How long a write waits for another writer's lock.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// The logs of a session: one opaque BLOB per log name.
const SCHEMA_LOGS: &str = "CREATE TABLE IF NOT EXISTS logs (\n    session_id TEXT NOT NULL,\n    name TEXT NOT NULL,\n    bytes BLOB NOT NULL,\n    PRIMARY KEY (session_id, name)\n)";

/// The files of a session: one opaque BLOB per file name.
const SCHEMA_FILES: &str = "CREATE TABLE IF NOT EXISTS files (\n    session_id TEXT NOT NULL,\n    name TEXT NOT NULL,\n    bytes BLOB NOT NULL,\n    PRIMARY KEY (session_id, name)\n)";

/// The tables of the session schema nu-agent kept before rig-memory.
const LEGACY_TABLES: &str =
    "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('sessions', 'entries')";

// endregion: --- Constants

// region:    --- Types

/// A rig-memory [`Store`] over a SQLite database, scoped to one session.
pub struct RigSqliteStore {
    session_id: String,
    jobs: Mutex<Option<mpsc::Sender<Request>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

/// What a job answers: its output, or why the database refused it.
type Reply = Result<Box<dyn Any + Send>, sqlx::Error>;

/// One call of the store, run on the worker thread.
struct Request {
    job: Box<dyn FnOnce(&Db) -> Reply + Send + 'static>,
    reply: mpsc::Sender<Reply>,
}

/// What the worker thread owns: the pool, and the runtime its calls run on.
struct Db {
    pool: SqlitePool,
    runtime: tokio::runtime::Runtime,
}

// endregion: --- Types

// region:    --- Constructors

impl RigSqliteStore {
    /// Opens the store of `session_id` at `path`, creating the database and
    /// its tables when they are missing.
    ///
    /// # Errors
    ///
    /// [`RigStoreError::Legacy`] when `path` holds the `sessions`/`entries`
    /// schema of an older nu-agent; that database is left as it is.
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
    /// [`RigStoreError::Legacy`] when `path` holds the `sessions`/`entries`
    /// schema of an older nu-agent; that database is left as it is.
    pub fn session_ids(path: &str) -> Result<Vec<String>, RigStoreError> {
        let (jobs, worker) = spawn_worker(path)?;
        let store = Self {
            session_id: String::new(),
            jobs: Mutex::new(Some(jobs)),
            worker: Mutex::new(Some(worker)),
        };
        store
            .call(|db| {
                db.run(async move {
                    let rows: Vec<(String,)> =
                        sqlx::query_as("SELECT DISTINCT session_id FROM logs ORDER BY session_id")
                            .fetch_all(&db.pool)
                            .await?;
                    Ok(rows.into_iter().map(|(id,)| id).collect::<Vec<_>>())
                })
            })
            .map_err(|error| RigStoreError::Io(io::Error::other(error)))
    }
}

// endregion: --- Constructors

// region:    --- Support

impl RigSqliteStore {
    /// Runs `job` on the worker thread and answers what it returned.
    fn call<T, F>(&self, job: F) -> Result<T, sqlx::Error>
    where
        T: Send + 'static,
        F: FnOnce(&Db) -> Result<T, sqlx::Error> + Send + 'static,
    {
        let (reply, answer) = mpsc::channel();
        let job: Box<dyn FnOnce(&Db) -> Reply + Send + 'static> = Box::new(move |db| {
            let out = job(db)?;
            Ok(Box::new(out) as Box<dyn Any + Send>)
        });
        self.jobs()
            .as_ref()
            .ok_or(sqlx::Error::WorkerCrashed)?
            .send(Request { job, reply })
            .map_err(|_| sqlx::Error::WorkerCrashed)?;
        let out = answer.recv().map_err(|_| sqlx::Error::WorkerCrashed)?;
        out?.downcast::<T>().map(|out| *out).map_err(|_| {
            sqlx::Error::Protocol("the store worker answered with another type".to_owned())
        })
    }

    fn jobs(&self) -> MutexGuard<'_, Option<mpsc::Sender<Request>>> {
        self.jobs.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Db {
    /// Runs `work` to its end on the worker thread's runtime.
    fn run<T>(&self, work: impl Future<Output = Result<T, sqlx::Error>>) -> Result<T, sqlx::Error> {
        self.runtime.block_on(work)
    }
}

/// Refuses `name` as a log name: a [`check_name`] in one part.
fn check_leaf(name: &str) -> io::Result<()> {
    check_name(name)?;
    if name.contains('/') {
        let why = format!("{name:?} is no log name");
        return Err(io::Error::new(io::ErrorKind::InvalidInput, why));
    }
    Ok(())
}

fn not_found() -> io::Error {
    io::ErrorKind::NotFound.into()
}

/// Spawns the worker thread and waits until it has opened the database.
fn spawn_worker(path: &str) -> Result<(mpsc::Sender<Request>, JoinHandle<()>), RigStoreError> {
    let (jobs, inbox) = mpsc::channel::<Request>();
    let (ready, opened) = mpsc::channel::<Result<(), RigStoreError>>();
    let path = path.to_owned();
    let worker = std::thread::Builder::new()
        .name("rig-sqlite-store".to_owned())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = ready.send(Err(RigStoreError::Io(error)));
                    return;
                }
            };
            let pool = match runtime.block_on(open_pool(&path)) {
                Ok(pool) => pool,
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            let db = Db { pool, runtime };
            if ready.send(Ok(())).is_err() {
                return;
            }
            while let Ok(request) = inbox.recv() {
                let out = (request.job)(&db);
                let _ = request.reply.send(out);
            }
        })
        .map_err(RigStoreError::Io)?;
    match opened.recv() {
        Ok(Ok(())) => Ok((jobs, worker)),
        Ok(Err(error)) => {
            let _ = worker.join();
            Err(error)
        }
        Err(_) => {
            let _ = worker.join();
            Err(RigStoreError::Io(io::Error::other(
                "the SQLite store worker stopped before it opened the database",
            )))
        }
    }
}

/// Opens the pool at `path`, refusing a legacy schema and creating the tables.
async fn open_pool(path: &str) -> Result<SqlitePool, RigStoreError> {
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
    refuse_legacy(&pool).await?;
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

/// Refuses a database that holds the session schema nu-agent kept before
/// rig-memory.
async fn refuse_legacy(pool: &SqlitePool) -> Result<(), RigStoreError> {
    let legacy: Vec<(String,)> = sqlx::query_as(LEGACY_TABLES).fetch_all(pool).await?;
    if legacy.is_empty() {
        Ok(())
    } else {
        Err(RigStoreError::Legacy)
    }
}

// endregion: --- Support

// region:    --- Files

impl Files for RigSqliteStore {
    fn write_file(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        check_name(name)?;
        let session = self.session_id.clone();
        let name = name.to_owned();
        let bytes = bytes.to_vec();
        self.call(move |db| {
            db.run(async move {
                sqlx::query(
                    "INSERT INTO files (session_id, name, bytes) VALUES (?1, ?2, ?3)\n                     ON CONFLICT(session_id, name) DO UPDATE SET bytes = excluded.bytes",
                )
                .bind(&session)
                .bind(&name)
                .bind(&bytes)
                .execute(&db.pool)
                .await?;
                Ok(())
            })
        })
        .map_err(io::Error::other)
    }

    fn append_file(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        check_name(name)?;
        let session = self.session_id.clone();
        let name = name.to_owned();
        let bytes = bytes.to_vec();
        self.call(move |db| {
            db.run(async move {
                sqlx::query(
                    "INSERT INTO files (session_id, name, bytes) VALUES (?1, ?2, ?3)\n                     ON CONFLICT(session_id, name)\n                     DO UPDATE SET bytes = CAST(bytes || excluded.bytes AS BLOB)",
                )
                .bind(&session)
                .bind(&name)
                .bind(&bytes)
                .execute(&db.pool)
                .await?;
                Ok(())
            })
        })
        .map_err(io::Error::other)
    }

    fn read_file(&self, name: &str) -> io::Result<Vec<u8>> {
        check_name(name)?;
        let session = self.session_id.clone();
        let name = name.to_owned();
        self.call(move |db| {
            db.run(async move {
                let row: Option<(Vec<u8>,)> =
                    sqlx::query_as("SELECT bytes FROM files WHERE session_id = ?1 AND name = ?2")
                        .bind(&session)
                        .bind(&name)
                        .fetch_optional(&db.pool)
                        .await?;
                Ok(row.map(|(bytes,)| bytes))
            })
        })
        .map_err(io::Error::other)?
        .ok_or_else(not_found)
    }

    fn remove_file(&self, name: &str) -> io::Result<()> {
        check_name(name)?;
        let session = self.session_id.clone();
        let name = name.to_owned();
        self.call(move |db| {
            db.run(async move {
                sqlx::query("DELETE FROM files WHERE session_id = ?1 AND name = ?2")
                    .bind(&session)
                    .bind(&name)
                    .execute(&db.pool)
                    .await?;
                Ok(())
            })
        })
        .map_err(io::Error::other)
    }

    fn files(&self) -> io::Result<Vec<String>> {
        let session = self.session_id.clone();
        self.call(move |db| {
            db.run(async move {
                let rows: Vec<(String,)> =
                    sqlx::query_as("SELECT name FROM files WHERE session_id = ?1 ORDER BY name")
                        .bind(&session)
                        .fetch_all(&db.pool)
                        .await?;
                Ok(rows.into_iter().map(|(name,)| name).collect::<Vec<_>>())
            })
        })
        .map_err(io::Error::other)
    }
}

// endregion: --- Files

// region:    --- Store

impl Store for RigSqliteStore {
    fn logs(&self) -> io::Result<Vec<String>> {
        let session = self.session_id.clone();
        self.call(move |db| {
            db.run(async move {
                let rows: Vec<(String,)> =
                    sqlx::query_as("SELECT name FROM logs WHERE session_id = ?1 ORDER BY name")
                        .bind(&session)
                        .fetch_all(&db.pool)
                        .await?;
                Ok(rows.into_iter().map(|(name,)| name).collect::<Vec<_>>())
            })
        })
        .map_err(io::Error::other)
    }

    fn read_log(&self, name: &str) -> io::Result<Vec<u8>> {
        check_leaf(name)?;
        let session = self.session_id.clone();
        let name = name.to_owned();
        self.call(move |db| {
            db.run(async move {
                let row: Option<(Vec<u8>,)> =
                    sqlx::query_as("SELECT bytes FROM logs WHERE session_id = ?1 AND name = ?2")
                        .bind(&session)
                        .bind(&name)
                        .fetch_optional(&db.pool)
                        .await?;
                Ok(row.map(|(bytes,)| bytes))
            })
        })
        .map_err(io::Error::other)?
        .ok_or_else(not_found)
    }

    fn append_log(&self, name: &str, len: u64, bytes: &[u8]) -> Result<(), LogWriteError> {
        check_leaf(name)?;
        let Ok(len) = i64::try_from(len) else {
            return Err(LogWriteError::Changed);
        };
        let session = self.session_id.clone();
        let name = name.to_owned();
        let bytes = bytes.to_vec();
        let written = self
            .call(move |db| {
                db.run(async move {
                    if len == 0 {
                        let started = sqlx::query(
                            "INSERT INTO logs (session_id, name, bytes) VALUES (?1, ?2, ?3)\n                             ON CONFLICT(session_id, name) DO NOTHING",
                        )
                        .bind(&session)
                        .bind(&name)
                        .bind(&bytes)
                        .execute(&db.pool)
                        .await?
                        .rows_affected();
                        if started > 0 {
                            return Ok(true);
                        }
                    }
                    let appended = sqlx::query(
                        "UPDATE logs SET bytes = CAST(bytes || ?3 AS BLOB)\n                         WHERE session_id = ?1 AND name = ?2\n                         AND length(CAST(bytes AS BLOB)) = ?4",
                    )
                    .bind(&session)
                    .bind(&name)
                    .bind(&bytes)
                    .bind(len)
                    .execute(&db.pool)
                    .await?
                    .rows_affected();
                    Ok(appended > 0)
                })
            })
            .map_err(|error| LogWriteError::Io(io::Error::other(error)))?;
        if written {
            Ok(())
        } else {
            Err(LogWriteError::Changed)
        }
    }

    fn replace_log(&self, name: &str, len: u64, bytes: &[u8]) -> Result<(), LogWriteError> {
        check_leaf(name)?;
        let Ok(len) = i64::try_from(len) else {
            return Err(LogWriteError::Changed);
        };
        let session = self.session_id.clone();
        let name = name.to_owned();
        let bytes = bytes.to_vec();
        let written = self
            .call(move |db| {
                db.run(async move {
                    if len == 0 {
                        let started = sqlx::query(
                            "INSERT INTO logs (session_id, name, bytes) VALUES (?1, ?2, ?3)\n                             ON CONFLICT(session_id, name) DO NOTHING",
                        )
                        .bind(&session)
                        .bind(&name)
                        .bind(&bytes)
                        .execute(&db.pool)
                        .await?
                        .rows_affected();
                        if started > 0 {
                            return Ok(true);
                        }
                    }
                    let replaced = sqlx::query(
                        "UPDATE logs SET bytes = ?3\n                         WHERE session_id = ?1 AND name = ?2\n                         AND length(CAST(bytes AS BLOB)) = ?4",
                    )
                    .bind(&session)
                    .bind(&name)
                    .bind(&bytes)
                    .bind(len)
                    .execute(&db.pool)
                    .await?
                    .rows_affected();
                    Ok(replaced > 0)
                })
            })
            .map_err(|error| LogWriteError::Io(io::Error::other(error)))?;
        if written {
            Ok(())
        } else {
            Err(LogWriteError::Changed)
        }
    }

    fn destroy(&self) -> io::Result<()> {
        let session = self.session_id.clone();
        self.call(move |db| {
            db.run(async move {
                let mut tx = db.pool.begin().await?;
                sqlx::query("DELETE FROM logs WHERE session_id = ?1")
                    .bind(&session)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("DELETE FROM files WHERE session_id = ?1")
                    .bind(&session)
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await
            })
        })
        .map_err(io::Error::other)
    }
}

// endregion: --- Store

// region:    --- Drop

impl Drop for RigSqliteStore {
    fn drop(&mut self) {
        let jobs = self
            .jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        drop(jobs);
        let worker = self
            .worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

// endregion: --- Drop
