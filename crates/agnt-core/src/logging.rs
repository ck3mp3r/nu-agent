//! The process file log sink.
//!
//! Both binaries log to the resolved `Paths.log_path` through [`FileLogSink`], a
//! writer whose target can be swapped once. The state ladder is readable from
//! the process environment, so the sink opens at process start; a
//! `[paths].state_home` value only becomes known after config.toml loads, so the
//! config-load site retargets the sink through [`retarget_process_sink`] and the
//! pre-load window stays in the derived default.

// region:    --- Modules

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

// endregion: --- Modules

// region:    --- Types

/// A cloneable, retargetable file log sink.
///
/// Every clone writes through the same handle, so the writer installed in the
/// logger and the handle the config-load site retargets are the same sink. A
/// write opens the target lazily and keeps it open until the target changes.
#[derive(Clone)]
pub struct FileLogSink {
    state: Arc<StdMutex<SinkState>>,
}

/// The current target and its open handle.
struct SinkState {
    path: PathBuf,
    file: Option<std::fs::File>,
}

// endregion: --- Types

// region:    --- Functions

impl FileLogSink {
    // --- Constructors ---

    /// Open `path` for appending, creating its parent directory.
    ///
    /// # Errors
    ///
    /// The error of the directory creation or of the file open, so the caller
    /// can fall back to a sink target.
    pub fn open(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let file = open_append(&path)?;
        Ok(Self {
            state: Arc::new(StdMutex::new(SinkState {
                path,
                file: Some(file),
            })),
        })
    }

    // --- Accessors ---

    /// The path writes currently go to.
    pub fn path(&self) -> PathBuf {
        let state = self.lock();
        state.path.clone()
    }

    // --- Mutators ---

    /// Retarget the sink. The next write appends to `path`.
    ///
    /// A target that cannot be opened is logged and the current one stays in
    /// use, so a bad `[paths]` value never stops the process from logging.
    pub fn retarget(&self, path: impl Into<PathBuf>) {
        let path = path.into();
        let mut state = self.lock();
        if state.path == path {
            return;
        }
        match open_append(&path) {
            Ok(file) => {
                state.path = path;
                state.file = Some(file);
            }
            Err(error) => log::warn!(
                "Failed to retarget the log file to {}: {error}",
                path.display()
            ),
        }
    }

    // --- Support ---

    /// Take the state lock, ignoring poisoning: a log write must never panic.
    fn lock(&self) -> std::sync::MutexGuard<'_, SinkState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
}

impl Write for FileLogSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut state = self.lock();
        let path = state.path.clone();
        let file = match &mut state.file {
            Some(file) => file,
            None => {
                state.file = Some(open_append(&path)?);
                state.file.as_mut().ok_or_else(|| {
                    io::Error::other(format!("failed to open the log file {}", path.display()))
                })?
            }
        };
        file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut state = self.lock();
        match &mut state.file {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

/// The process log sink, set once by the binary that installs the logger.
static PROCESS_SINK: OnceLock<FileLogSink> = OnceLock::new();

/// Install `sink` as the process log sink so a later config load can retarget
/// it. A second install is ignored.
pub fn install_process_sink(sink: &FileLogSink) {
    let _ = PROCESS_SINK.set(sink.clone());
}

/// Retarget the process log sink, when one is installed.
///
/// The config-load site calls this with the fully resolved
/// [`Paths::log_path`](crate::config::Paths::log_path); a process without an
/// installed sink (a test, or a binary that logs elsewhere) ignores it.
pub fn retarget_process_sink(path: &Path) {
    if let Some(sink) = PROCESS_SINK.get() {
        sink.retarget(path.to_path_buf());
    }
}

// endregion: --- Functions

// region:    --- Support

/// Open `path` for appending, creating its parent directory.
fn open_append(path: &Path) -> io::Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../test/logging.rs"]
mod logging_test;

// endregion: --- Tests
