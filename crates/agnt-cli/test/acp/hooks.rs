//! ACP agent-lifecycle hook tests.
//!
//! The agent process publishes one `AgentEvent::Started`/`Shutdown` pair per
//! process lifetime. These tests configure a real `nu` hook script in the
//! subprocess's config.toml and assert the envelopes the script writes, so the
//! ACP emission criteria are verified end to end rather than by code reading.
//!
//! The process is driven directly over stdio rather than through the SDK
//! client: the SDK kills the child when its connection drops, so a client-driven
//! test can never observe the agent's own clean exit.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use super::*;

// region:    --- Test Support

/// The `[[hooks]]` entry the ACP subprocess resolves against its config dir.
const AGENT_HOOK_CONFIG: &str = "[[hooks]]\nname = \"audit\"\nevents = [\"agent_started\", \"agent_shutdown\"]\nscript = \"audit.nu\"";

/// A minimal `initialize` request: enough to prove the server is serving.
const INITIALIZE_REQUEST: &str = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":1,\"clientCapabilities\":{}}}";

/// How long the hook log may take to reach its final state.
const HOOK_LOG_TIMEOUT: Duration = Duration::from_secs(10);

/// The lines of the hook log, or an empty vec when the file does not exist.
fn hook_log_lines(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .map(|body| body.lines().map(|line| line.to_string()).collect())
        .unwrap_or_default()
}

/// Poll the hook log until every event in `events` appears, or fail.
///
/// The agent drains its hook runners before the process exits, but the file is
/// written by a subprocess, so the log is polled rather than read once.
fn wait_for_hook_events(log: &Path, events: &[&str]) -> Result<Vec<String>> {
    let deadline = Instant::now() + HOOK_LOG_TIMEOUT;
    loop {
        let lines = hook_log_lines(log);
        let missing: Vec<&str> = events
            .iter()
            .copied()
            .filter(|event| {
                !lines
                    .iter()
                    .any(|line| line.contains(&format!("\"{event}\"")))
            })
            .collect();
        if missing.is_empty() {
            return Ok(lines);
        }
        if Instant::now() >= deadline {
            return Err(format!("hook log never reported {missing:?}; got: {lines:?}").into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

// endregion: --- Test Support

// region:    --- Tests

#[test]
fn test_integration_acp_emits_agent_lifecycle_envelopes() -> Result<()> {
    // -- Setup & Fixtures
    let env = TestEnv::new("http://127.0.0.1:1/v1", AGENT_HOOK_CONFIG)?;
    let log = env.temp_path().join("agent-hook.log");
    env.write_agent_hook_script("audit.nu", &log)?;

    // -- Exec
    let mut child = env
        .command()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or("child stdin must be piped")?;
    let stdout = child.stdout.take().ok_or("child stdout must be piped")?;
    let mut stdout = BufReader::new(stdout);
    writeln!(stdin, "{INITIALIZE_REQUEST}")?;
    stdin.flush()?;
    let mut response = String::new();
    stdout.read_line(&mut response)?;
    // Closing stdin is the client disconnect: the server sees EOF, publishes
    // Shutdown, drains its hook runners, then exits.
    drop(stdin);
    let status = child.wait()?;

    // -- Check
    assert!(
        response.contains("\"protocolVersion\""),
        "the server must answer initialize; got: {response:?}"
    );
    assert!(status.success(), "the server must exit cleanly: {status}");
    let lines = wait_for_hook_events(&log, &["agent_started", "agent_shutdown"])?;
    assert!(
        lines.iter().any(|line| line.contains("\"mode\": \"acp\"")),
        "agent_started must carry mode acp; got: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("\"reason\": \"shutdown\"")),
        "agent_shutdown must carry reason shutdown on a clean exit; got: {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("\"version\": 1")),
        "the envelope must carry the v1 header; got: {lines:?}"
    );
    Ok(())
}

// endregion: --- Tests
