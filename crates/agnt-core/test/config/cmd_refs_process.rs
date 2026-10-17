//! Integration tests for [`ProcessCmdRunner`].
//!
//! These tests spawn real child processes, so they are gated behind the
//! `integration` feature.

use std::time::{Duration, Instant};

use super::{CmdRunError, CmdRunner, ProcessCmdRunner};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_cmd_refs_process_echo_returns_stdout() -> Result<()> {
    // -- Setup & Fixtures
    let runner = ProcessCmdRunner::default();

    // -- Exec
    let stdout = runner.run("echo", &["hello"])?;

    // -- Check
    assert_eq!(stdout.trim_end_matches(['\n', '\r']), "hello");
    Ok(())
}

#[test]
fn test_cmd_refs_process_non_zero_exit_reports_code_and_stderr() -> Result<()> {
    // -- Setup & Fixtures
    let runner = ProcessCmdRunner::default();

    // -- Exec
    let error = match runner.run("sh", &["-c", "echo SECRET_STDOUT; echo boom >&2; exit 3"]) {
        Ok(_) => return Err("a non-zero exit must fail".into()),
        Err(error) => error,
    };

    // -- Check
    let message = error.to_string();
    assert!(message.contains('3'), "must name the exit code: {message}");
    assert!(message.contains("boom"), "must include stderr: {message}");
    assert!(
        !message.contains("SECRET_STDOUT"),
        "must not leak stdout: {message}"
    );
    Ok(())
}

#[test]
fn test_cmd_refs_process_missing_program_reports_io_error() -> Result<()> {
    // -- Setup & Fixtures
    let runner = ProcessCmdRunner::default();

    // -- Exec
    let error = match runner.run("nu-agent-no-such-command", &[]) {
        Ok(_) => return Err("a missing program must fail".into()),
        Err(error) => error,
    };

    // -- Check
    assert!(
        matches!(&error, CmdRunError::Io(_)),
        "expected an I/O error, got: {error}"
    );
    Ok(())
}

#[test]
fn test_cmd_refs_process_timeout_kills_sleeping_child() -> Result<()> {
    // -- Setup & Fixtures
    let runner = ProcessCmdRunner::new(Duration::from_millis(200));
    let started = Instant::now();

    // -- Exec
    let error = match runner.run("sleep", &["5"]) {
        Ok(_) => return Err("a sleeping child must time out".into()),
        Err(error) => error,
    };

    // -- Check
    assert!(
        matches!(&error, CmdRunError::Timeout(_)),
        "expected a timeout, got: {error}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the timeout must fire promptly"
    );
    Ok(())
}

#[test]
fn test_cmd_refs_process_timeout_with_continuous_output() -> Result<()> {
    // -- Setup & Fixtures
    let runner = ProcessCmdRunner::new(Duration::from_millis(200));
    let started = Instant::now();

    // -- Exec
    let error = match runner.run("yes", &[]) {
        Ok(_) => return Err("a continuously writing child must time out".into()),
        Err(error) => error,
    };

    // -- Check
    assert!(
        matches!(&error, CmdRunError::Timeout(_)),
        "expected a timeout, got: {error}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the timeout must fire promptly"
    );
    Ok(())
}

// endregion: --- Tests
