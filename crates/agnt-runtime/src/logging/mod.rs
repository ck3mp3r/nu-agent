//! The process file log sink and its session ring.
//!
//! Both binaries log to the resolved `Paths.log_path` through [`FileLogSink`], a
//! writer whose target can be swapped once. The state ladder is readable from
//! the process environment, so the sink opens at process start; a
//! `[paths].state_home` value only becomes known after config.toml loads, so the
//! config-load site retargets the sink through [`retarget_process_sink`] and the
//! pre-load window stays in the derived default.
//!
//! The file is the only target: a path that does not resolve or does not open
//! falls back to a per-process file in the temp directory, so a release binary
//! never drops a log line and never aborts a run for the sink. Beside it, the
//! [`LogRing`] keeps the most recent lines in memory, and the ring is appended to
//! the file on quit and on panic.

// region:    --- Modules

/// The ring beside the file: the last lines a run produced, kept in memory.
mod ring;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, Once, OnceLock};

use ring::LogRing;

// endregion: --- Modules

// region:    --- Constants

/// File name prefix of the last-resort log file in the temp directory.
const FALLBACK_LOG_PREFIX: &str = "nu-agent-log";

/// Number of previous session logs kept beside the current one.
const KEPT_SESSIONS: u8 = 3;

// endregion: --- Constants

// region:    --- Types

/// A cloneable, retargetable file log sink.
///
/// Every clone writes through the same handle, so the writer installed in the
/// logger and the handle the config-load site retargets are the same sink. A
/// write opens the target lazily and keeps it open until the target changes.
#[derive(Clone)]
struct FileLogSink {
    state: Arc<StdMutex<SinkState>>,
}

/// The current target, its open handle, and whether the target is silenced.
struct SinkState {
    path: PathBuf,
    file: Option<std::fs::File>,
    /// Set while the run asks for silence: writes are accepted and discarded,
    /// so nothing reaches the file.
    silenced: bool,
}

/// A bounded buffer of the most recent formatted log lines.
/// The logger target: every line goes to the file sink and into the ring.
///
/// The ring holds whole lines: a line split across two writes is joined before
/// it is recorded. Level filtering happens above this writer — the logger's
/// filter, floored at warn, decides what arrives here.
struct LoggingWriter {
    sink: FileLogSink,
    ring: Arc<LogRing>,
    partial: String,
}

/// The sink a process start opened, and the reason it fell back when it did.
struct OpenedLog {
    sink: FileLogSink,
    fallback: Option<String>,
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
                silenced: false,
            })),
        })
    }

    /// A sink that opens `path` on its first write.
    ///
    /// The last resort: a target that cannot open here still carries the write
    /// attempt instead of failing the caller.
    pub fn lazy(path: impl Into<PathBuf>) -> Self {
        Self {
            state: Arc::new(StdMutex::new(SinkState {
                path: path.into(),
                file: None,
                silenced: false,
            })),
        }
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

    /// Silence or unsilence the file target.
    ///
    /// A silenced sink accepts every write and discards it, so the writer in
    /// front of it records the line in the ring as it would when audible: that
    /// is what makes `off` mean "no file, ring intact".
    pub fn set_silenced(&self, silenced: bool) {
        self.lock().silenced = silenced;
    }

    /// Append `bytes` to the target whatever the silence says.
    ///
    /// [`LogRing::flush_into`](super::ring::LogRing::flush_into) is the one
    /// caller: the dump is the whole reason a silenced run still records, so
    /// obeying the silence here would leave that run with an empty file and its
    /// evidence dropped. The exemption lives in the write path itself — flipping
    /// `silenced` around the dump would race every other writer.
    fn write_unsilenced(&self, bytes: &[u8]) -> io::Result<()> {
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

impl LoggingWriter {
    // --- Constructors ---

    /// A target that writes to `sink` and records whole lines into `ring`.
    fn new(sink: FileLogSink, ring: Arc<LogRing>) -> Self {
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
    fn record_fallback(&self, reason: &str) {
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

/// The process log sink, set once by the binary that installs the logger.
static PROCESS_SINK: OnceLock<FileLogSink> = OnceLock::new();

/// The process log ring, set once by [`start_session_log`].
static PROCESS_RING: OnceLock<Arc<LogRing>> = OnceLock::new();

/// Install `sink` as the process log sink so a later config load can retarget
/// it. A second install is ignored.
fn install_process_sink(sink: &FileLogSink) {
    let _ = PROCESS_SINK.set(sink.clone());
}

/// Install `ring` as the process log ring. A second install is ignored.
fn install_process_ring(ring: &Arc<LogRing>) {
    let _ = PROCESS_RING.set(Arc::clone(ring));
}

/// Retarget the process log sink, when one is installed.
///
/// The config-load site calls this with the fully resolved
/// `Paths::log_path`; a process without an installed sink (a test, or a binary
/// that logs elsewhere) ignores it.
pub fn retarget_process_sink(path: &Path) {
    if let Some(sink) = PROCESS_SINK.get() {
        sink.retarget(path.to_path_buf());
    }
}

/// Start the session log and return the logger target that writes to it.
///
/// `path` is the resolved `Paths::log_path`, or `None` when the state ladder
/// does not resolve. The previous runs rotate out of the way first, so each
/// process start keeps its own contiguous block of lines. A resolved path that
/// cannot open falls back to a per-process file in the temp directory: logging
/// never lands on stderr and never fails the caller.
///
/// The first call in a process installs the sink, the ring, and the panic hook;
/// a later call reuses them, so a long-lived plugin process logs into one file
/// handle and one ring.
pub fn start_session_log(path: Option<&Path>) -> impl Write + use<> {
    let (Some(sink), Some(ring)) = (PROCESS_SINK.get(), PROCESS_RING.get()) else {
        let opened = open_session_sink(path);
        install_process_sink(&opened.sink);
        let ring = Arc::new(LogRing::default());
        install_process_ring(&ring);
        install_process_panic_hook(&opened.sink, &ring);
        let writer = LoggingWriter::new(opened.sink, ring);
        if let Some(reason) = &opened.fallback {
            writer.record_fallback(reason);
        }
        return writer;
    };
    LoggingWriter::new(sink.clone(), Arc::clone(ring))
}

/// Append the process ring to the process sink and sync it.
///
/// Both binaries call this when their run ends, so the lines a crash or a quit
/// cut short are still on disk.
pub fn flush_ring_to_file() {
    let (Some(sink), Some(ring)) = (PROCESS_SINK.get(), PROCESS_RING.get()) else {
        return;
    };
    ring.flush_into(sink);
}

/// Raise `level` to warn when it sits below.
///
/// The file and the ring must always receive warn and above, whatever
/// `RUST_LOG` or a flag asks for; the level above warn keeps the environment's
/// own choice. [`resolved_level`] is the one public entry: a binary names the
/// level it wants and gets the logger level and the sink mode together.
fn floor_at_warn(level: log::LevelFilter) -> log::LevelFilter {
    level.max(log::LevelFilter::Warn)
}

/// The logger level to install, and whether the file target is silenced.
///
/// `Off` is a sink mode, not a logger level. The ring records what the logger
/// receives, so an explicit `off` keeps the logger at warn — the ring still
/// gathers warn and above — and silences the file instead. Every other level
/// floors at warn and leaves the file audible.
pub fn resolved_level(requested: log::LevelFilter) -> (log::LevelFilter, bool) {
    match requested {
        log::LevelFilter::Off => (log::LevelFilter::Warn, true),
        level => (floor_at_warn(level), false),
    }
}

/// Silence or unsilence the process log file, when a sink is installed.
///
/// The binary that resolved its level calls this once, right after the logger
/// is installed: the sink must exist before it can be silenced.
pub fn set_process_sink_silenced(silenced: bool) {
    if let Some(sink) = PROCESS_SINK.get() {
        sink.set_silenced(silenced);
    }
}

/// Install a panic hook that appends the ring to `sink` before the standard
/// hook runs.
///
/// The dump goes to the file only, never to the terminal: the TUI restores the
/// terminal around a panic, and a line written here would land on the restored
/// screen. The hook therefore stays off the display path entirely.
fn install_process_panic_hook(sink: &FileLogSink, ring: &Arc<LogRing>) {
    static HOOK: Once = Once::new();
    let sink = sink.clone();
    let ring = Arc::clone(ring);
    HOOK.call_once(move || {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            ring.flush_into(&sink);
            previous(info);
        }));
    });
}

// endregion: --- Functions

// region:    --- Support

/// Open the session sink, rotating the previous runs out of the way first.
///
/// An unresolvable or unwritable target falls back to a per-process temp file
/// together with the reason it did.
fn open_session_sink(path: Option<&Path>) -> OpenedLog {
    let Some(path) = path else {
        return fallback_sink("the state ladder did not resolve".to_string());
    };
    rotate_session_logs(path);
    match FileLogSink::open(path) {
        Ok(sink) => OpenedLog {
            sink,
            fallback: None,
        },
        Err(error) => fallback_sink(format!("{}: {error}", path.display())),
    }
}

/// Open the per-process fallback log in the temp directory.
fn fallback_sink(reason: String) -> OpenedLog {
    let path =
        std::env::temp_dir().join(format!("{FALLBACK_LOG_PREFIX}-{}.log", std::process::id()));
    match FileLogSink::open(&path) {
        Ok(sink) => OpenedLog {
            sink,
            fallback: Some(reason),
        },
        Err(_) => OpenedLog {
            sink: FileLogSink::lazy(path),
            fallback: Some(reason),
        },
    }
}

/// Rotate `path` out to `path.1` and shift the kept sessions down.
///
/// Each process start gets its own block of lines: `agent.log` holds the
/// current run and `agent.log.1` .. `agent.log.3` the three previous ones. A
/// rename or a removal that fails is ignored, so an unrotatable file appends in
/// place instead of failing the run.
fn rotate_session_logs(path: &Path) {
    let _ = std::fs::remove_file(session_path(path, KEPT_SESSIONS));
    for index in (1..KEPT_SESSIONS).rev() {
        let from = session_path(path, index);
        if from.exists() {
            let _ = std::fs::rename(&from, session_path(path, index + 1));
        }
    }
    let _ = std::fs::rename(path, session_path(path, 1));
}

/// The rotated name of `path` for session `index`.
fn session_path(path: &Path, index: u8) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{index}"));
    PathBuf::from(name)
}

/// The sink's open target, opened on first use.
///
/// One place opens the file, so the silenced write path, the unsilenced dump and
/// the lazy sink all share the same open-or-create step.
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

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/logging.rs"]
mod logging_test;

// endregion: --- Tests
