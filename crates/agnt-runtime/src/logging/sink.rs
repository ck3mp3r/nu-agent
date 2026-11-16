//! The file sink behind the logger, and the writer that tees its lines into the
//! ring.

// region:    --- Modules

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

use super::ring::LogRing;

// endregion: --- Modules

// region:    --- Types

/// A cloneable, retargetable file log sink.
///
/// Every clone writes through the same handle, so the writer installed in the
/// logger and the handle a config-load site retargets are the same sink. A write
/// opens the target lazily and keeps it open until the target changes.
#[derive(Clone)]
pub(super) struct FileLogSink {
    state: Arc<StdMutex<SinkState>>,
    /// Why the sink is the last-resort one, when it is.
    notice: Option<String>,
}

/// The current target, its open handle, and whether the target is silenced.
struct SinkState {
    path: PathBuf,
    file: Option<std::fs::File>,
    /// Set while the run asks for silence: writes are accepted and discarded,
    /// so nothing reaches the file.
    silenced: bool,
}

/// The logger target: every line goes to the file sink and into the ring.
///
/// The ring holds whole lines: a line split across two writes is joined before
/// it is recorded. Level filtering happens above this writer — the logger's
/// filter, floored at warn, decides what arrives here.
pub(super) struct LoggingWriter {
    sink: FileLogSink,
    ring: Arc<LogRing>,
    partial: String,
}

// endregion: --- Types

// region:    --- FileLogSink

impl FileLogSink {
    // --- Constructors ---

    /// Open `path` for appending, creating its parent directory.
    ///
    /// # Errors
    ///
    /// The error of the directory creation or of the file open, so the caller
    /// can fall back to a sink target.
    pub(super) fn open(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let file = open_append(&path)?;
        Ok(Self {
            state: Arc::new(StdMutex::new(SinkState {
                path,
                file: Some(file),
                silenced: false,
            })),
            notice: None,
        })
    }

    /// Open `path` for appending; a target that cannot open is carried instead.
    ///
    /// The last resort: a sink that cannot open here still carries the write
    /// attempt instead of failing the caller, and keeps `notice` as the reason
    /// the run fell back to it.
    pub(super) fn fallback(path: impl Into<PathBuf>, notice: String) -> Self {
        let path = path.into();
        let state = match open_append(&path) {
            Ok(file) => SinkState {
                path,
                file: Some(file),
                silenced: false,
            },
            Err(_) => SinkState {
                path,
                file: None,
                silenced: false,
            },
        };
        Self {
            state: Arc::new(StdMutex::new(state)),
            notice: Some(notice),
        }
    }

    // --- Accessors ---

    /// The path writes currently go to.
    pub(super) fn path(&self) -> PathBuf {
        self.lock().path.clone()
    }

    /// Why the sink is the last-resort one, when it is.
    pub(super) fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    // --- Mutators ---

    /// Retarget the sink. The next write appends to `path`.
    ///
    /// A target that cannot be opened is logged and the current one stays in
    /// use, so a bad `[paths]` value never stops the process from logging.
    pub(super) fn retarget(&self, path: impl Into<PathBuf>) {
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

    /// Retarget the sink into `dir`, keeping the file name it writes under.
    ///
    /// The instance identity is fixed at install time: a `[paths].state_home`
    /// value moves the directory, never the instance's own file name.
    pub(super) fn retarget_dir(&self, dir: &Path) {
        let name = self.path().file_name().map(|name| name.to_os_string());
        if let Some(name) = name {
            self.retarget(dir.join(name));
        }
    }

    /// Silence or unsilence the file target.
    ///
    /// A silenced sink accepts every write and discards it, so the writer in
    /// front of it records the line in the ring as it would when audible: that
    /// is what makes `off` mean "no file, ring intact".
    pub(super) fn set_silenced(&self, silenced: bool) {
        self.lock().silenced = silenced;
    }

    /// Append `bytes` to the target whatever the silence says.
    ///
    /// [`LogRing::flush_into`](super::ring::LogRing::flush_into) is the one
    /// caller: the dump is the whole reason a silenced run still records, so
    /// obeying the silence here would leave that run with an empty file and its
    /// evidence dropped. The exemption lives in the write path itself — flipping
    /// `silenced` around the dump would race every other writer.
    pub(super) fn write_unsilenced(&self, bytes: &[u8]) -> io::Result<()> {
        let mut state = self.lock();
        let file = open_target(&mut state)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()
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
        // Silenced: accept the write and keep nothing, so the ring records the
        // line while the file stays empty.
        if state.silenced {
            return Ok(buf.len());
        }
        open_target(&mut state)?.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut state = self.lock();
        if state.silenced {
            return Ok(());
        }
        match &mut state.file {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

// endregion: --- FileLogSink

// region:    --- LoggingWriter

impl LoggingWriter {
    // --- Constructors ---

    /// A target that writes to `sink` and records whole lines into `ring`.
    pub(super) fn new(sink: FileLogSink, ring: Arc<LogRing>) -> Self {
        Self {
            sink,
            ring,
            partial: String::new(),
        }
    }

    // --- Mutators ---

    /// Note the fallback as the first line of the ring.
    ///
    /// The logger is not installed yet, so this line cannot travel through it;
    /// the ring dump puts it in the fallback file on quit and on panic.
    pub(super) fn record_fallback(&self, reason: &str) {
        self.ring.push(&format!(
            "log sink fell back to {}: {reason}",
            self.sink.path().display()
        ));
    }

    // --- Support ---

    /// Record the whole lines of `bytes`; a trailing fragment waits for more.
    fn record(&mut self, bytes: &[u8]) {
        self.partial.push_str(&String::from_utf8_lossy(bytes));
        while let Some(end) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=end).collect();
            let line = line.trim_end();
            if !line.is_empty() {
                self.ring.push(line);
            }
        }
    }

    /// Record a fragment a flush ends without a newline.
    fn record_partial(&mut self) {
        let line = std::mem::take(&mut self.partial);
        let line = line.trim_end();
        if !line.is_empty() {
            self.ring.push(line);
        }
    }
}

impl Write for LoggingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.sink.write(buf)?;
        if let Some(accepted) = buf.get(..written) {
            self.record(accepted);
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.record_partial();
        self.sink.flush()
    }
}

// endregion: --- LoggingWriter

// region:    --- Support

/// The sink's open target, opened on first use.
///
/// One place opens the file, so the silenced write path, the unsilenced dump and
/// a fallback sink all share the same open-or-create step.
fn open_target(state: &mut SinkState) -> io::Result<&mut std::fs::File> {
    let path = state.path.clone();
    if state.file.is_none() {
        state.file = Some(open_append(&path)?);
    }
    state
        .file
        .as_mut()
        .ok_or_else(|| io::Error::other(format!("failed to open the log file {}", path.display())))
}

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
