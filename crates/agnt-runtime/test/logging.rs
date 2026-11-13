//! Tests for the retargetable file log sink, the session ring, and its dump.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use log::LevelFilter;

use super::ring::RING_CAPACITY;
use super::{
    FileLogSink, LogRing, LoggingWriter, floor_at_warn, flush_ring_to_file,
    install_process_panic_hook, install_process_ring, install_process_sink, open_session_sink,
    resolved_level,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_logging_sink_writes_to_the_opened_path() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("logs").join("agent.log");
    let mut sink = FileLogSink::open(path.clone())?;

    // -- Exec
    sink.write_all(b"first line\n")?;
    sink.flush()?;

    // -- Check
    assert_eq!(sink.path(), path);
    assert_eq!(std::fs::read_to_string(&path)?, "first line\n");
    Ok(())
}

#[test]
fn test_logging_sink_retarget_writes_to_the_new_path() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let derived = temp.path().join("state").join("logs").join("agent.log");
    let from_config = temp
        .path()
        .join("from-config")
        .join("logs")
        .join("agent.log");
    let mut sink = FileLogSink::open(derived.clone())?;
    sink.write_all(b"pre-load\n")?;
    sink.flush()?;

    // -- Exec
    sink.retarget(from_config.clone());
    sink.write_all(b"post-load\n")?;
    sink.flush()?;

    // -- Check: the pre-load window kept the derived default
    assert_eq!(sink.path(), from_config);
    assert_eq!(std::fs::read_to_string(&derived)?, "pre-load\n");
    assert_eq!(std::fs::read_to_string(&from_config)?, "post-load\n");
    Ok(())
}

#[test]
fn test_logging_sink_keeps_the_current_target_when_retarget_fails() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("agent.log");
    let mut sink = FileLogSink::open(path.clone())?;
    sink.write_all(b"kept\n")?;
    sink.flush()?;
    // A directory where the file should be: opening it fails.
    let blocked = temp.path().join("blocked");
    std::fs::create_dir_all(&blocked)?;

    // -- Exec
    sink.retarget(blocked.clone());
    sink.write_all(b"still here\n")?;
    sink.flush()?;

    // -- Check: the current target stays in use
    assert_eq!(sink.path(), path);
    assert_eq!(std::fs::read_to_string(&path)?, "kept\nstill here\n");
    Ok(())
}

#[test]
fn test_logging_ring_keeps_the_last_capacity_lines() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("agent.log");
    let ring = LogRing::default();
    for index in 0..RING_CAPACITY + 50 {
        ring.push(&format!("line {index}"));
    }

    // -- Exec
    ring.flush_into(&FileLogSink::open(path.clone())?);

    // -- Check: the oldest 50 lines are gone, the marker closes the dump
    let content = std::fs::read_to_string(&path)?;
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), RING_CAPACITY + 1);
    assert_eq!(lines.first().copied(), Some("line 50"));
    assert_eq!(
        lines.last().copied(),
        Some("---- ring dump (last 500 lines) ----")
    );
    Ok(())
}

#[test]
fn test_logging_ring_flush_writes_nothing_when_empty() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("agent.log");
    let ring = LogRing::default();

    // -- Exec
    ring.flush_into(&FileLogSink::open(path.clone())?);

    // -- Check: an empty ring adds no marker line
    assert_eq!(std::fs::read_to_string(&path)?, "");
    Ok(())
}

#[test]
fn test_logging_writer_tees_complete_lines_to_the_sink_and_the_ring() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("agent.log");
    let sink = FileLogSink::open(path.clone())?;
    let ring = Arc::new(LogRing::default());
    let mut writer = LoggingWriter::new(sink, Arc::clone(&ring));

    // -- Exec: two lines in one write, then one line split across two writes
    writer.write_all(b"first line\nsecond ")?;
    writer.write_all(b"line\n")?;
    writer.flush()?;

    // -- Check: the sink holds the bytes verbatim, the ring whole lines only
    assert_eq!(std::fs::read_to_string(&path)?, "first line\nsecond line\n");
    let dump_path = temp.path().join("dump.log");
    ring.flush_into(&FileLogSink::open(dump_path.clone())?);
    assert_eq!(
        std::fs::read_to_string(&dump_path)?,
        "first line\nsecond line\n---- ring dump (last 2 lines) ----\n"
    );
    Ok(())
}

#[test]
fn test_logging_floor_at_warn_raises_only_levels_below_warn() -> Result<()> {
    // -- Exec & Check
    for level in [LevelFilter::Off, LevelFilter::Error, LevelFilter::Warn] {
        assert_eq!(floor_at_warn(level), LevelFilter::Warn, "{level:?}");
    }
    for level in [LevelFilter::Info, LevelFilter::Debug, LevelFilter::Trace] {
        assert_eq!(floor_at_warn(level), level, "{level:?}");
    }
    Ok(())
}

#[test]
fn test_logging_open_session_sink_rotates_the_previous_runs() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("logs").join("agent.log");
    std::fs::create_dir_all(path.parent().ok_or("the log path has a parent")?)?;
    std::fs::write(&path, "current run\n")?;
    std::fs::write(rotated(&path, 1), "session one\n")?;
    std::fs::write(rotated(&path, 2), "session two\n")?;
    std::fs::write(rotated(&path, 3), "session three\n")?;

    // -- Exec
    let opened = open_session_sink(Some(&path));

    // -- Check: three previous runs kept, the oldest dropped
    assert!(opened.fallback.is_none(), "the resolved path opens");
    assert_eq!(opened.sink.path(), path);
    assert_eq!(std::fs::read_to_string(&path)?, "");
    assert_eq!(std::fs::read_to_string(rotated(&path, 1))?, "current run\n");
    assert_eq!(std::fs::read_to_string(rotated(&path, 2))?, "session one\n");
    assert_eq!(std::fs::read_to_string(rotated(&path, 3))?, "session two\n");
    assert!(
        !rotated(&path, 4).exists(),
        "the fourth previous run is dropped"
    );
    Ok(())
}

#[test]
fn test_logging_open_session_sink_falls_back_to_a_temp_file() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    // A file where the log directory should be: creating the directory fails.
    let blocked = temp.path().join("blocked");
    std::fs::write(&blocked, "")?;
    let path = blocked.join("logs").join("agent.log");

    // -- Exec
    let opened = open_session_sink(Some(&path));
    let fallback = opened.sink.path();

    // -- Check: a per-process temp file instead of a failed run
    assert!(opened.fallback.is_some(), "an unwritable path falls back");
    assert_ne!(fallback, path);
    assert_eq!(fallback.parent(), Some(std::env::temp_dir().as_path()));
    let name = fallback
        .file_name()
        .ok_or("the fallback has a file name")?
        .to_string_lossy()
        .to_string();
    assert!(
        name.starts_with("nu-agent-log-") && name.ends_with(".log"),
        "{name}"
    );
    assert!(fallback.exists(), "the fallback file is open");
    let _ = std::fs::remove_file(&fallback);
    Ok(())
}

#[test]
fn test_logging_panic_hook_dumps_the_ring_before_the_previous_hook() -> Result<()> {
    // -- Setup & Fixtures: a previous hook that appends its own witness line
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("agent.log");
    let sink = FileLogSink::open(path.clone())?;
    let ring = Arc::new(LogRing::default());
    ring.push("buffered before the panic");
    let previous = std::panic::take_hook();
    let witness = path.clone();
    std::panic::set_hook(Box::new(move |_| {
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&witness)
            .and_then(|mut file| file.write_all(b"previous hook\n"));
    }));
    install_process_panic_hook(&sink, &ring);

    // -- Exec
    let panicked = std::panic::catch_unwind(|| panic!("boom")).is_err();
    std::panic::set_hook(previous);

    // -- Check: the ring lands first, the chained hook still runs
    assert!(panicked, "the panic propagates");
    let content = std::fs::read_to_string(&path)?;
    let dump = content
        .find("buffered before the panic")
        .ok_or("the ring line reaches the file")?;
    let chained = content
        .find("previous hook")
        .ok_or("the previous hook still runs")?;
    assert!(dump < chained, "the ring lands before the previous hook");
    Ok(())
}

#[test]
fn test_logging_flush_ring_to_file_dumps_the_installed_ring() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("agent.log");
    let sink = FileLogSink::open(path.clone())?;
    let ring = Arc::new(LogRing::default());
    ring.push("kept until quit");
    install_process_sink(&sink);
    install_process_ring(&ring);

    // -- Exec
    flush_ring_to_file();

    // -- Check
    assert_eq!(
        std::fs::read_to_string(&path)?,
        "kept until quit\n---- ring dump (last 1 lines) ----\n"
    );
    Ok(())
}

#[test]
fn test_logging_writer_records_the_fallback_as_the_first_ring_line() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("nu-agent-log-1.log");
    let sink = FileLogSink::open(path.clone())?;
    let ring = Arc::new(LogRing::default());
    let writer = LoggingWriter::new(sink.clone(), Arc::clone(&ring));

    // -- Exec
    writer.record_fallback("/blocked/logs/agent.log: Not a directory");
    ring.flush_into(&sink);

    // -- Check: the notice names the file and the reason, ahead of any other line
    let content = std::fs::read_to_string(&path)?;
    let expected = format!(
        "log sink fell back to {}: /blocked/logs/agent.log: Not a directory",
        path.display()
    );
    assert_eq!(content.lines().next(), Some(expected.as_str()));
    Ok(())
}

// region:    --- Test Support

/// The rotated name of `path` for session `index`.
fn rotated(path: &Path, index: u8) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{index}"));
    PathBuf::from(name)
}

// endregion: --- Test Support

// region:    --- Off semantics

/// WHEN the level resolves to `off`, THEN the logger SHALL stay at warn for the
/// ring and the file SHALL be the silenced target — for `--log-level off` and
/// for `RUST_LOG=off`, which parses to the same level.
#[test]
fn test_logging_resolved_level_off_silences_the_sink_and_keeps_the_logger_at_warn() -> Result<()> {
    // -- Exec & Check
    assert_eq!(
        resolved_level(LevelFilter::Off),
        (LevelFilter::Warn, true),
        "off keeps the ring's warn floor and silences the file"
    );
    assert_eq!(
        "off".parse::<LevelFilter>()?,
        LevelFilter::Off,
        "RUST_LOG's off directive is the same level the flag names"
    );
    for level in [
        LevelFilter::Error,
        LevelFilter::Warn,
        LevelFilter::Info,
        LevelFilter::Debug,
        LevelFilter::Trace,
    ] {
        assert_eq!(
            resolved_level(level),
            (floor_at_warn(level), false),
            "{level:?} floors at warn and stays audible"
        );
    }
    Ok(())
}

/// WHEN the file target is silenced, THEN it SHALL accept every write and keep
/// none, while the ring in front of it records the lines as usual.
#[test]
fn test_logging_silenced_sink_keeps_nothing_while_the_ring_records() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("agent.log");
    let sink = FileLogSink::open(path.clone())?;
    let ring = Arc::new(LogRing::default());
    let mut writer = LoggingWriter::new(sink.clone(), Arc::clone(&ring));
    writer.write_all(b"audible line\n")?;
    writer.flush()?;

    // -- Exec: silence the file and log the line the ring must still hold.
    sink.set_silenced(true);
    writer.write_all(b"warn while off\n")?;
    writer.flush()?;

    // -- Check: the file kept the line before the silence and nothing after it.
    assert_eq!(std::fs::read_to_string(&path)?, "audible line\n");
    let dump_path = temp.path().join("dump.log");
    ring.flush_into(&FileLogSink::open(dump_path.clone())?);
    let dumped = std::fs::read_to_string(&dump_path)?;
    assert!(
        dumped.contains("warn while off"),
        "the ring records what the silenced file drops: {dumped}"
    );

    // -- Exec: a later line is audible again.
    sink.set_silenced(false);
    writer.write_all(b"audible again\n")?;
    writer.flush()?;

    // -- Check
    assert_eq!(
        std::fs::read_to_string(&path)?,
        "audible line\naudible again\n",
        "unsilencing resumes the file"
    );
    Ok(())
}

/// WHEN a silenced run ends, THEN its ring dump SHALL land in the same sink —
/// the lines were gathered for exactly that moment, so the dump is the one
/// write the silence does not cover.
///
/// This is the production shape: one sink, silenced, fed through the writer, and
/// dumped into itself on quit. A dump into a fresh sink would pass on the
/// broken code and prove nothing.
#[test]
fn test_logging_silenced_run_dumps_the_ring_into_its_own_sink() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("agent.log");
    let sink = FileLogSink::open(path.clone())?;
    let ring = Arc::new(LogRing::default());
    let mut writer = LoggingWriter::new(sink.clone(), Arc::clone(&ring));
    sink.set_silenced(true);

    // -- Exec: a warning reaches the logger target, then the run ends.
    writer.write_all(b"[WARN] wedged mcp toggle: alpha\n")?;
    writer.flush()?;
    ring.flush_into(&sink);

    // -- Check: the file holds the dump and nothing else — no formatted lines,
    // and not a zero-byte file either.
    let content = std::fs::read_to_string(&path)?;
    assert_eq!(
        content, "[WARN] wedged mcp toggle: alpha\n---- ring dump (last 1 lines) ----\n",
        "the silenced run keeps its formatted lines out and its dump in"
    );
    Ok(())
}

// endregion: --- Off semantics
