//! The instance log: the logs directory a process owns, the identity that names
//! its instance, and the process sink the logger writes through.
//!
//! Each process writes one instance file — `agent-{name}-{pid}.log`, and
//! `agent-{pid}.log` without a name — so a second instance starting never
//! touches the file an earlier one is writing. The pid is always part of the
//! name: two same-named concurrent instances never interleave into one file. The
//! role (`tui`, `a2a`, `acp`, `run`) is recorded in the file's header lines,
//! never in its name, so the identity stays app-level rather than surface-level.
//! The state ladder is readable from the process environment, so the store opens
//! at process start; a `[paths].state_home` value only becomes known after
//! config.toml loads, so the config-load site retargets the sink into the
//! resolved `logs/` directory and the pre-load window stays in the derived
//! default.
//!
//! The file is the only target: a path that does not open falls back to a
//! per-process file in the temp directory, so a release binary never drops a log
//! line and never aborts a run for the sink. Beside it, the [`LogRing`] keeps
//! the most recent lines in memory, and the ring is appended to the instance's
//! own file on quit and on panic.

// region:    --- Modules

/// The ring beside the file: the last lines a run produced, kept in memory.
mod ring;

/// The file sink behind the logger, and the writer that tees lines into the ring.
mod sink;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Once, OnceLock};
use std::time::{Duration, SystemTime};

use ring::LogRing;
use sink::{FileLogSink, LoggingWriter};

// endregion: --- Modules

// region:    --- Constants

/// File name prefix of the last-resort log file in the temp directory.
const FALLBACK_LOG_PREFIX: &str = "nu-agent-log";

/// Number of old instance files the store keeps beside the running instance.
const KEPT_INSTANCE_LOGS: usize = 20;

/// Prefix of every instance log file.
const INSTANCE_LOG_PREFIX: &str = "agent-";

/// Suffix of every instance log file.
const INSTANCE_LOG_SUFFIX: &str = ".log";

/// An instance file modified this recently is never collected: a writer that
/// froze leaves a recent trail, and that trail is the evidence a collection
/// exists to preserve.
const RECENT_LOG_WINDOW: Duration = Duration::from_secs(60 * 60);

/// Why a store has no state tree to write in.
const UNRESOLVED_LADDER: &str = "the state ladder did not resolve";

// endregion: --- Constants

// region:    --- Types

/// The role an instance plays, recorded in its log's header lines.
///
/// The role never names the file: every instance file is
/// `agent-{name}-{pid}.log`, so two instances never compose the same path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogRole {
    /// The interaction route: TUI, TTY, and single-prompt runs.
    Tui,
    /// The A2A agent server route, in both its TUI and stderr modes.
    A2a,
    /// The Agent Client Protocol server route.
    Acp,
    /// A one-shot run: the CLI's utility subcommands and the plugin pipeline.
    Run,
}

impl LogRole {
    /// The token the header records for this role.
    fn token(self) -> &'static str {
        match self {
            Self::Tui => "tui",
            Self::A2a => "a2a",
            Self::Acp => "acp",
            Self::Run => "run",
        }
    }
}

/// The identity of one logging instance.
///
/// The identity is fixed when the log starts: a run never renames its own file.
/// `name` is the instance's name (`--name`); without one the pid alone names the
/// instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogIdentity {
    role: LogRole,
    name: Option<String>,
    pid: u32,
}

impl LogIdentity {
    // --- Constructors ---

    /// The identity of this process in `role`.
    pub fn new(role: LogRole) -> Self {
        Self {
            role,
            name: None,
            pid: std::process::id(),
        }
    }

    // --- Chainable setters ---

    /// Name the instance.
    ///
    /// The name is a file name part, never a path: anything outside the file
    /// name's own alphabet becomes `-`, and an empty name leaves the instance
    /// named by its pid alone.
    pub fn with_name(mut self, name: impl Into<Option<String>>) -> Self {
        self.name = name
            .into()
            .map(|name| sanitized(&name))
            .filter(|name| !name.is_empty());
        self
    }

    // --- Accessors ---

    /// The file name this identity composes to.
    fn file_name(&self) -> String {
        let pid = self.pid;
        match &self.name {
            Some(name) => format!("{INSTANCE_LOG_PREFIX}{name}-{pid}{INSTANCE_LOG_SUFFIX}"),
            None => format!("{INSTANCE_LOG_PREFIX}{pid}{INSTANCE_LOG_SUFFIX}"),
        }
    }
}

/// The owner of the logs directory.
///
/// The store composes each instance's file name, opens the instance's own sink,
/// and collects old instance files at open time.
#[derive(Clone, Debug)]
pub struct LogStore {
    dir: PathBuf,
    /// Why the store is the last-resort one, when it is.
    notice: Option<String>,
}

/// The process's one instance log: the sink the logger writes to and the ring
/// beside it.
struct ProcessLog {
    sink: FileLogSink,
    ring: Arc<LogRing>,
}

// endregion: --- Types

// region:    --- LogStore

impl LogStore {
    // --- Constructors ---

    /// The store over `dir`, the logs directory of the resolved state tree.
    pub fn open(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            notice: None,
        }
    }

    /// The last-resort store: the temp directory, with the reason recorded.
    ///
    /// A process whose state ladder does not resolve still logs — into
    /// `{temp dir}/nu-agent-log-{pid}.log` — and the first ring line names why.
    pub fn fallback() -> Self {
        Self {
            dir: std::env::temp_dir(),
            notice: Some(UNRESOLVED_LADDER.to_string()),
        }
    }

    // --- Accessors ---

    /// The path `identity`'s file occupies in this store's directory.
    fn instance_file(&self, identity: &LogIdentity) -> PathBuf {
        self.dir.join(identity.file_name())
    }

    // --- Mutators ---

    /// Start the instance log and return the logger target that writes to it.
    ///
    /// The first call in a process collects old instance files, opens the
    /// instance's own file, writes the identity header, installs the ring, and
    /// installs the panic hook. A later call reuses them, so a long-lived process
    /// logs into one file handle and one ring, and its header is written once.
    pub fn start(&self, identity: &LogIdentity) -> impl Write + use<> {
        if let Some(process) = PROCESS_LOG.get() {
            return LoggingWriter::new(process.sink.clone(), Arc::clone(&process.ring));
        }
        self.gc(identity);
        let sink = self.session_sink(identity);
        // The header is identity, not log volume: it lands ahead of any log line
        // whatever the resolved level asks for, so an `off` run's file still
        // names the instance that wrote it.
        let _ = sink.write_unsilenced(instance_header(identity).as_bytes());
        let ring = Arc::new(LogRing::default());
        install_process_panic_hook(&sink, &ring);
        let writer = LoggingWriter::new(sink.clone(), Arc::clone(&ring));
        if let Some(notice) = sink.notice() {
            writer.record_fallback(notice);
        }
        let _ = PROCESS_LOG.set(ProcessLog { sink, ring });
        writer
    }

    /// Point the installed process sink into `dir`, keeping its file name.
    ///
    /// The config-load site calls this with the resolved `Paths::log_dir`: the
    /// directory moves, the instance's own file name does not. A process without
    /// an installed sink (a test, or a binary that logs elsewhere) ignores it.
    pub fn retarget_process(dir: &Path) {
        if let Some(process) = PROCESS_LOG.get() {
            process.sink.retarget_dir(dir);
        }
    }

    /// Silence or unsilence the process log file, when a sink is installed.
    ///
    /// The binary that resolved its level calls this once, right after the
    /// logger is installed: the sink must exist before it can be silenced.
    pub fn set_process_silenced(silenced: bool) {
        if let Some(process) = PROCESS_LOG.get() {
            process.sink.set_silenced(silenced);
        }
    }

    /// Append the process ring to the instance file and sync it.
    ///
    /// Both binaries call this when their run ends, so the lines a crash or a
    /// quit cut short are still on disk.
    pub fn flush_process_ring() {
        if let Some(process) = PROCESS_LOG.get() {
            process.ring.flush_into(&process.sink);
        }
    }

    /// The instance file the process sink currently writes, once one exists.
    ///
    /// The path follows a retarget: a `[paths].state_home` value moves the
    /// directory and keeps the file name. A process without an installed sink —
    /// a test, or a binary that logs elsewhere — has no instance file.
    pub fn instance_path() -> Option<PathBuf> {
        PROCESS_LOG.get().map(|process| process.sink.path())
    }

    // --- Support ---

    /// Compose and open the instance file, falling back to the temp file.
    ///
    /// A store that already fell back keeps its notice; a resolved directory
    /// whose instance file will not open hands its own reason to the sink.
    fn session_sink(&self, identity: &LogIdentity) -> FileLogSink {
        if let Some(notice) = &self.notice {
            return FileLogSink::fallback(fallback_path(), notice.clone());
        }
        let instance = self.instance_file(identity);
        match FileLogSink::open(&instance) {
            Ok(sink) => sink,
            Err(error) => {
                FileLogSink::fallback(fallback_path(), format!("{}: {error}", instance.display()))
            }
        }
    }

    /// Collect old instance files: keep the newest [`KEPT_INSTANCE_LOGS`].
    ///
    /// Order every instance file by mtime and drop the ones past the newest
    /// twenty. Three files are never collected: the owner's own file, a file
    /// another live instance wrote within [`RECENT_LOG_WINDOW`], and a file the
    /// platform refuses to remove (skipped silently — a collection never fails
    /// the run that opened the log).
    fn gc(&self, owner: &LogIdentity) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let own = self.instance_file(owner);
        let now = SystemTime::now();
        let mut collected: Vec<(SystemTime, PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path == own || !is_instance_file(&path) {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let Ok(modified) = metadata.modified() else {
                continue;
            };
            if now.duration_since(modified).unwrap_or_default() < RECENT_LOG_WINDOW {
                continue;
            }
            collected.push((modified, path));
        }
        collected.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        for (_, path) in collected.into_iter().skip(KEPT_INSTANCE_LOGS) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The process instance log, set once when the log starts.
static PROCESS_LOG: OnceLock<ProcessLog> = OnceLock::new();

// endregion: --- LogStore

// region:    --- Functions

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

/// The name as a file name: every character outside the file name's own
/// alphabet becomes `-`, so a name can never reach a path.
fn sanitized(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

/// The header block an instance file opens with.
///
/// The block is the identity: the name when the run carries one, the pid, the
/// role the run plays, and the start time. It is composed once per process, so a
/// long-lived process writes it once.
fn instance_header(identity: &LogIdentity) -> String {
    let mut header = String::from("---- instance log ----\n");
    if let Some(name) = &identity.name {
        header.push_str(&format!("name: {name}\n"));
    }
    let pid = identity.pid;
    let role = identity.role.token();
    let started = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    header.push_str(&format!("pid: {pid}\n"));
    header.push_str(&format!("role: {role}\n"));
    header.push_str(&format!("started: {started}\n"));
    header
}

/// The last-resort log file for this process.
fn fallback_path() -> PathBuf {
    std::env::temp_dir().join(format!("{FALLBACK_LOG_PREFIX}-{}.log", std::process::id()))
}

/// Whether `path` names an instance log file.
///
/// A legacy `tui-*`/`a2a-*` file does not match: the collection stops seeing it
/// and leaves it where it is.
fn is_instance_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with(INSTANCE_LOG_PREFIX) && name.ends_with(INSTANCE_LOG_SUFFIX)
        })
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/logging/sink.rs"]
mod sink_test;

#[cfg(test)]
#[path = "../../test/logging/store.rs"]
mod store_test;

// endregion: --- Tests
