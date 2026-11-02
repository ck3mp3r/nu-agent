//! The storage handle: the engine thread, and the two faces of its transport.

// region:    --- Modules

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use crate::engine::{self, Inbox, Location, Work, WorkUnit};
use crate::error::{Result, StorageError};

// endregion: --- Modules

// region:    --- Types

/// One database, served by one engine thread.
///
/// Every SQL statement runs on that thread: a caller submits a [`Work`] unit
/// and receives the value the unit answers. [`Self::call`] is the async face,
/// [`Self::call_blocking`] the sync face.
pub struct Storage {
    inbox: Mutex<Option<Inbox>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

// endregion: --- Types

// region:    --- Constructors

impl Storage {
    /// Opens the database at `path`, creating the file, its parent
    /// directories, and its schema when they are missing.
    ///
    /// The path `:memory:` keeps the database in memory, like
    /// [`Storage::in_memory`].
    ///
    /// # Errors
    ///
    /// [`StorageError::SchemaTooNew`] when the database was written by a newer
    /// build, [`StorageError::Io`] or [`StorageError::Sqlite`] when it cannot
    /// be created, opened, or migrated.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::start(Location::at(path.as_ref()))
    }

    /// Opens a database in memory: nothing touches the filesystem.
    ///
    /// # Errors
    ///
    /// [`StorageError::Sqlite`] when the database cannot be opened or
    /// migrated.
    pub fn in_memory() -> Result<Self> {
        Self::start(Location::Memory)
    }

    /// Starts the engine of `location` and waits until it has opened the
    /// database.
    fn start(location: Location) -> Result<Self> {
        let (inbox, worker) = engine::spawn(location)?;
        Ok(Self {
            inbox: Mutex::new(Some(inbox)),
            worker: Mutex::new(Some(worker)),
        })
    }
}

// endregion: --- Constructors

// region:    --- Transport

impl Storage {
    /// Submits `work` and awaits the value it answers.
    ///
    /// # Errors
    ///
    /// [`StorageError::EngineClosed`] when the engine is gone,
    /// [`StorageError::EngineStopped`] when it stopped before it answered.
    pub async fn call<W: Work>(&self, work: W) -> Result<W::Output> {
        self.submit(work).await
    }

    /// Submits `work` and blocks until the value it answers.
    ///
    /// # Errors
    ///
    /// [`StorageError::EngineClosed`] when the engine is gone,
    /// [`StorageError::EngineStopped`] when it stopped before it answered.
    ///
    /// The wait is a plain channel receive, so it works on a thread that
    /// drives asynchronous tasks too: the caller blocks that thread until the
    /// engine answers. Prefer [`Self::call`] where an await is possible.
    pub fn call_blocking<W: Work>(&self, work: W) -> Result<W::Output> {
        let (answer, unit) = engine::boxed_blocking(work);
        self.deliver(unit)?;
        answer.recv().map_err(|_| StorageError::EngineStopped)
    }

    /// Submits `work` to the engine, awaiting the value it answers.
    async fn submit<W: Work>(&self, work: W) -> Result<W::Output> {
        let (answer, unit) = engine::boxed(work);
        self.deliver(unit)?;
        answer.await.map_err(|_| StorageError::EngineStopped)
    }

    /// Hands `unit` to the engine's inbox.
    fn deliver(&self, unit: Box<dyn WorkUnit>) -> Result<()> {
        self.inbox()
            .as_ref()
            .ok_or(StorageError::EngineClosed)?
            .send(unit)
            .map_err(|_| StorageError::EngineClosed)
    }

    fn inbox(&self) -> MutexGuard<'_, Option<Inbox>> {
        self.inbox.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

// endregion: --- Transport

// region:    --- Drop

impl Drop for Storage {
    fn drop(&mut self) {
        let inbox = self
            .inbox
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        drop(inbox);
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
