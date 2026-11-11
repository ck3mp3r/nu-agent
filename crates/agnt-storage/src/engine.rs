//! The engine thread: it owns the runtime and the pool, and it runs every unit
//! of SQL work this crate performs.

// region:    --- Modules

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use tokio::sync::oneshot;

use crate::error::{Result, StorageError};

// endregion: --- Modules

// region:    --- Constants

/// The path that keeps the database in memory.
const IN_MEMORY: &str = ":memory:";

/// How long a statement waits for another connection's lock.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// The connections a file database opens: one statement runs at a time, and a
/// transaction that also reads through the pool needs a second connection.
const FILE_CONNECTIONS: u32 = 4;

/// The connections an in-memory database opens: the pool holds the database,
/// so it stays at the one connection that holds it.
const MEMORY_CONNECTIONS: u32 = 1;

/// The migrations this build embeds, and the only owner of the schema.
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

// endregion: --- Constants

// region:    --- Types

/// One unit of SQL work a caller submits to the engine, and the value it
/// answers.
pub trait Work: Send + 'static {
    /// The value the unit answers its caller.
    type Output: Send + 'static;

    /// Runs the unit against `pool`, on the engine thread.
    fn run(self, pool: SqlitePool) -> impl Future<Output = Self::Output> + Send;
}

/// The engine's inbox: the one shape that crosses the thread boundary.
pub(crate) type Inbox = mpsc::Sender<Box<dyn WorkUnit>>;

/// What the engine thread owns: the pool, and the runtime its statements run
/// on.
pub(crate) struct Engine {
    pool: SqlitePool,
    runtime: tokio::runtime::Runtime,
}

/// Where a database lives.
#[derive(Debug)]
pub(crate) enum Location {
    /// In memory: nothing touches the filesystem.
    Memory,
    /// At a file path, created when it is missing.
    File(PathBuf),
}

/// The erased unit the engine runs: it carries its work and the sender it
/// answers, so the engine never needs the caller's types.
pub(crate) trait WorkUnit: Send + 'static {
    fn run(self: Box<Self>, engine: &Engine);
}

/// One [`Work`] unit and the sender it answers.
struct Envelope<W: Work> {
    work: W,
    reply: oneshot::Sender<W::Output>,
}

/// One [`Work`] unit whose caller waits on a plain channel, so a thread that
/// drives asynchronous tasks can block on it.
struct BlockingEnvelope<W: Work> {
    work: W,
    reply: mpsc::Sender<W::Output>,
}

// endregion: --- Types

// region:    --- Support

impl Location {
    /// The location of `path`: `:memory:` keeps the database in memory.
    pub(crate) fn at(path: &Path) -> Self {
        if path == Path::new(IN_MEMORY) {
            Self::Memory
        } else {
            Self::File(path.to_path_buf())
        }
    }

    fn connections(&self) -> u32 {
        match self {
            Self::Memory => MEMORY_CONNECTIONS,
            Self::File(_) => FILE_CONNECTIONS,
        }
    }

    fn connect_options(&self) -> SqliteConnectOptions {
        let options = match self {
            Self::Memory => SqliteConnectOptions::new().in_memory(true),
            Self::File(path) => SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true),
        };
        options
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(BUSY_TIMEOUT)
    }
}

impl Engine {
    fn block_on<F: Future>(&self, work: F) -> F::Output {
        self.runtime.block_on(work)
    }
}

impl<W: Work> WorkUnit for Envelope<W> {
    fn run(self: Box<Self>, engine: &Engine) {
        let Self { work, reply } = *self;
        let output = engine.block_on(work.run(engine.pool.clone()));
        let _ = reply.send(output);
    }
}

impl<W: Work> WorkUnit for BlockingEnvelope<W> {
    fn run(self: Box<Self>, engine: &Engine) {
        let Self { work, reply } = *self;
        let output = engine.block_on(work.run(engine.pool.clone()));
        let _ = reply.send(output);
    }
}

/// Pairs `work` with the sender its caller awaits.
pub(crate) fn boxed<W: Work>(work: W) -> (oneshot::Receiver<W::Output>, Box<dyn WorkUnit>) {
    let (reply, answer) = oneshot::channel();
    (answer, Box::new(Envelope { work, reply }))
}

/// Pairs `work` with the sender a blocking caller receives from.
pub(crate) fn boxed_blocking<W: Work>(work: W) -> (mpsc::Receiver<W::Output>, Box<dyn WorkUnit>) {
    let (reply, answer) = mpsc::channel();
    (answer, Box::new(BlockingEnvelope { work, reply }))
}

/// Opens the database of `location`, migrates it, and serves its work on a
/// thread of its own.
pub(crate) fn spawn(location: Location) -> Result<(Inbox, JoinHandle<()>)> {
    let (inbox, work) = mpsc::channel::<Box<dyn WorkUnit>>();
    let (ready, opened) = mpsc::channel::<Result<()>>();
    let worker = std::thread::Builder::new()
        .name("agnt-storage-engine".to_owned())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = ready.send(Err(StorageError::Io(error)));
                    return;
                }
            };
            let pool = match runtime.block_on(open_pool(&location)) {
                Ok(pool) => pool,
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            let engine = Engine { pool, runtime };
            log::debug!("the storage engine opened {location:?}");
            if ready.send(Ok(())).is_err() {
                return;
            }
            while let Ok(unit) = work.recv() {
                unit.run(&engine);
            }
            log::debug!("the storage engine stopped {location:?}");
        })
        .map_err(StorageError::Io)?;
    match opened.recv() {
        Ok(Ok(())) => Ok((inbox, worker)),
        Ok(Err(error)) => {
            let _ = worker.join();
            Err(error)
        }
        Err(_) => {
            let _ = worker.join();
            Err(StorageError::Custom(
                "the storage engine stopped before it opened the database".to_owned(),
            ))
        }
    }
}

/// Opens the pool of `location` and runs this build's migrations on it.
async fn open_pool(location: &Location) -> Result<SqlitePool> {
    if let Location::File(path) = location
        && let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let pool = SqlitePoolOptions::new()
        .max_connections(location.connections())
        .min_connections(1)
        .connect_with(location.connect_options())
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

// endregion: --- Support
