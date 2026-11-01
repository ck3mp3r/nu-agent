// region:    --- Modules

use std::future::Future;
use std::io;
use std::sync::mpsc;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use sqlx::sqlite::SqlitePool;

use super::open::open_pool;
use crate::session::factory::RigStoreError;

// endregion: --- Modules

// region:    --- Constants

/// The logs of a session: one opaque BLOB per log name.
pub(super) const SCHEMA_LOGS: &str = "CREATE TABLE IF NOT EXISTS logs (\n    session_id TEXT NOT NULL,\n    name TEXT NOT NULL,\n    bytes BLOB NOT NULL,\n    PRIMARY KEY (session_id, name)\n)";

/// The files of a session: one opaque BLOB per file name.
pub(super) const SCHEMA_FILES: &str = "CREATE TABLE IF NOT EXISTS files (\n    session_id TEXT NOT NULL,\n    name TEXT NOT NULL,\n    bytes BLOB NOT NULL,\n    PRIMARY KEY (session_id, name)\n)";

// endregion: --- Constants

// region:    --- Types

/// A rig-memory [`Store`](rig_memory::Store) over a SQLite database, scoped to one session.
pub struct RigSqliteStore {
    pub(super) session_id: String,
    pub(super) jobs: Mutex<Option<mpsc::Sender<Request>>>,
    pub(super) worker: Mutex<Option<JoinHandle<()>>>,
}

/// One operation of the store, carrying what the worker needs to run it.
pub(super) enum Job {
    WriteFile {
        session: String,
        name: String,
        bytes: Vec<u8>,
    },
    AppendFile {
        session: String,
        name: String,
        bytes: Vec<u8>,
    },
    ReadFile {
        session: String,
        name: String,
    },
    RemoveFile {
        session: String,
        name: String,
    },
    Files {
        session: String,
    },
    Logs {
        session: String,
    },
    ReadLog {
        session: String,
        name: String,
    },
    AppendLog {
        session: String,
        name: String,
        len: i64,
        bytes: Vec<u8>,
    },
    ReplaceLog {
        session: String,
        name: String,
        len: i64,
        bytes: Vec<u8>,
    },
    Destroy {
        session: String,
    },
    SessionIds,
}

/// What a [`Job`] answers: the shape its operation produces, or why the
/// database refused it.
enum Reply {
    /// The operations that only write: `write_file`, `append_file`,
    /// `remove_file`, and `destroy`.
    Unit(Result<(), sqlx::Error>),
    /// The operations that read one row: `read_file` and `read_log`.
    Bytes(Result<Option<Vec<u8>>, sqlx::Error>),
    /// The operations that list names: `files`, `logs`, and `session_ids`.
    Names(Result<Vec<String>, sqlx::Error>),
    /// The compare-and-set log writes: `append_log` and `replace_log`.
    Written(Result<bool, sqlx::Error>),
}

/// One call of the store: its job, and where the worker answers it.
pub(super) struct Request {
    job: Job,
    reply: mpsc::Sender<Reply>,
}

/// What the worker thread owns: the pool, and the runtime its calls run on.
struct Db {
    pool: SqlitePool,
    runtime: tokio::runtime::Runtime,
}

// endregion: --- Types

// region:    --- Support

impl RigSqliteStore {
    /// Sends `job` to the worker thread and answers what it replied.
    fn submit(&self, job: Job) -> Result<Reply, sqlx::Error> {
        let (reply, answer) = mpsc::channel();
        self.jobs()
            .as_ref()
            .ok_or(sqlx::Error::WorkerCrashed)?
            .send(Request { job, reply })
            .map_err(|_| sqlx::Error::WorkerCrashed)?;
        answer.recv().map_err(|_| sqlx::Error::WorkerCrashed)
    }

    /// Submits a job that only writes.
    pub(super) fn submit_unit(&self, job: Job) -> Result<(), sqlx::Error> {
        match self.submit(job)? {
            Reply::Unit(out) => out,
            _ => Err(answered_another_job()),
        }
    }

    /// Submits a job that reads one row, answering `None` when it is missing.
    pub(super) fn submit_row(&self, job: Job) -> Result<Option<Vec<u8>>, sqlx::Error> {
        match self.submit(job)? {
            Reply::Bytes(out) => out,
            _ => Err(answered_another_job()),
        }
    }

    /// Submits a job that lists names.
    pub(super) fn submit_names(&self, job: Job) -> Result<Vec<String>, sqlx::Error> {
        match self.submit(job)? {
            Reply::Names(out) => out,
            _ => Err(answered_another_job()),
        }
    }

    /// Submits a log write, answering whether it landed.
    pub(super) fn submit_written(&self, job: Job) -> Result<bool, sqlx::Error> {
        match self.submit(job)? {
            Reply::Written(out) => out,
            _ => Err(answered_another_job()),
        }
    }

    fn jobs(&self) -> MutexGuard<'_, Option<mpsc::Sender<Request>>> {
        self.jobs.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The error of a reply that carries another job's result: unreachable while
/// [`Db::handle`] answers every arm with its own shape, kept so that a mistake
/// fails loudly instead of silently.
fn answered_another_job() -> sqlx::Error {
    sqlx::Error::Protocol("the store worker answered another job's result".to_owned())
}

impl Db {
    /// Runs `work` to its end on the worker thread's runtime.
    fn run<T>(&self, work: impl Future<Output = Result<T, sqlx::Error>>) -> Result<T, sqlx::Error> {
        self.runtime.block_on(work)
    }

    /// Runs one job on the pool and answers it in the shape of its operation.
    fn handle(&self, job: Job) -> Reply {
        match job {
            Job::WriteFile { session, name, bytes } => Reply::Unit(self.run(async move {
                sqlx::query(
                    "INSERT INTO files (session_id, name, bytes) VALUES (?1, ?2, ?3)\n                     ON CONFLICT(session_id, name) DO UPDATE SET bytes = excluded.bytes",
                )
                .bind(&session)
                .bind(&name)
                .bind(&bytes)
                .execute(&self.pool)
                .await?;
                Ok(())
            })),
            Job::AppendFile { session, name, bytes } => Reply::Unit(self.run(async move {
                sqlx::query(
                    "INSERT INTO files (session_id, name, bytes) VALUES (?1, ?2, ?3)\n                     ON CONFLICT(session_id, name)\n                     DO UPDATE SET bytes = CAST(bytes || excluded.bytes AS BLOB)",
                )
                .bind(&session)
                .bind(&name)
                .bind(&bytes)
                .execute(&self.pool)
                .await?;
                Ok(())
            })),
            Job::ReadFile { session, name } => Reply::Bytes(self.run(async move {
                let row: Option<(Vec<u8>,)> =
                    sqlx::query_as("SELECT bytes FROM files WHERE session_id = ?1 AND name = ?2")
                        .bind(&session)
                        .bind(&name)
                        .fetch_optional(&self.pool)
                        .await?;
                Ok(row.map(|(bytes,)| bytes))
            })),
            Job::RemoveFile { session, name } => Reply::Unit(self.run(async move {
                sqlx::query("DELETE FROM files WHERE session_id = ?1 AND name = ?2")
                    .bind(&session)
                    .bind(&name)
                    .execute(&self.pool)
                    .await?;
                Ok(())
            })),
            Job::Files { session } => Reply::Names(self.run(async move {
                let rows: Vec<(String,)> =
                    sqlx::query_as("SELECT name FROM files WHERE session_id = ?1 ORDER BY name")
                        .bind(&session)
                        .fetch_all(&self.pool)
                        .await?;
                Ok(rows.into_iter().map(|(name,)| name).collect::<Vec<_>>())
            })),
            Job::Logs { session } => Reply::Names(self.run(async move {
                let rows: Vec<(String,)> =
                    sqlx::query_as("SELECT name FROM logs WHERE session_id = ?1 ORDER BY name")
                        .bind(&session)
                        .fetch_all(&self.pool)
                        .await?;
                Ok(rows.into_iter().map(|(name,)| name).collect::<Vec<_>>())
            })),
            Job::ReadLog { session, name } => Reply::Bytes(self.run(async move {
                let row: Option<(Vec<u8>,)> =
                    sqlx::query_as("SELECT bytes FROM logs WHERE session_id = ?1 AND name = ?2")
                        .bind(&session)
                        .bind(&name)
                        .fetch_optional(&self.pool)
                        .await?;
                Ok(row.map(|(bytes,)| bytes))
            })),
            Job::AppendLog { session, name, len, bytes } => Reply::Written(self.run(async move {
                if len == 0 {
                    let started = sqlx::query(
                        "INSERT INTO logs (session_id, name, bytes) VALUES (?1, ?2, ?3)\n                         ON CONFLICT(session_id, name) DO NOTHING",
                    )
                    .bind(&session)
                    .bind(&name)
                    .bind(&bytes)
                    .execute(&self.pool)
                    .await?
                    .rows_affected();
                    if started > 0 {
                        return Ok(true);
                    }
                }
                let appended = sqlx::query(
                    "UPDATE logs SET bytes = CAST(bytes || ?3 AS BLOB)\n                     WHERE session_id = ?1 AND name = ?2\n                     AND length(CAST(bytes AS BLOB)) = ?4",
                )
                .bind(&session)
                .bind(&name)
                .bind(&bytes)
                .bind(len)
                .execute(&self.pool)
                .await?
                .rows_affected();
                Ok(appended > 0)
            })),
            Job::ReplaceLog { session, name, len, bytes } => Reply::Written(self.run(async move {
                if len == 0 {
                    let started = sqlx::query(
                        "INSERT INTO logs (session_id, name, bytes) VALUES (?1, ?2, ?3)\n                         ON CONFLICT(session_id, name) DO NOTHING",
                    )
                    .bind(&session)
                    .bind(&name)
                    .bind(&bytes)
                    .execute(&self.pool)
                    .await?
                    .rows_affected();
                    if started > 0 {
                        return Ok(true);
                    }
                }
                let replaced = sqlx::query(
                    "UPDATE logs SET bytes = ?3\n                     WHERE session_id = ?1 AND name = ?2\n                     AND length(CAST(bytes AS BLOB)) = ?4",
                )
                .bind(&session)
                .bind(&name)
                .bind(&bytes)
                .bind(len)
                .execute(&self.pool)
                .await?
                .rows_affected();
                Ok(replaced > 0)
            })),
            Job::Destroy { session } => Reply::Unit(self.run(async move {
                let mut tx = self.pool.begin().await?;
                sqlx::query("DELETE FROM logs WHERE session_id = ?1")
                    .bind(&session)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("DELETE FROM files WHERE session_id = ?1")
                    .bind(&session)
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await
            })),
            Job::SessionIds => Reply::Names(self.run(async move {
                let rows: Vec<(String,)> =
                    sqlx::query_as("SELECT DISTINCT session_id FROM logs ORDER BY session_id")
                        .fetch_all(&self.pool)
                        .await?;
                Ok(rows.into_iter().map(|(id,)| id).collect::<Vec<_>>())
            })),
        }
    }
}

/// Spawns the worker thread and waits until it has opened the database.
pub(super) fn spawn_worker(
    path: &str,
) -> Result<(mpsc::Sender<Request>, JoinHandle<()>), RigStoreError> {
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
                let _ = request.reply.send(db.handle(request.job));
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

// endregion: --- Support

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
