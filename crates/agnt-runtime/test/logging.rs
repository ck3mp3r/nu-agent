//! Tests for the retargetable file log sink.

use std::io::Write;

use super::FileLogSink;

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
