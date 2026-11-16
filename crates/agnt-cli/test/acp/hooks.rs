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

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::*;

// region:    --- Test Support

/// The `[[hooks]]` entry the ACP subprocess resolves against its config dir.
const AGENT_HOOK_CONFIG: &str = "[[hooks]]\nname = \"audit\"\nevents = [\"agent_started\", \"agent_shutdown\"]\nscript = \"audit.nu\"";

/// A minimal `initialize` request: enough to prove the server is serving.
const INITIALIZE_REQUEST: &str = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":1,\"clientCapabilities\":{}}}";

/// How long the hook log may take to reach its final state.
const HOOK_LOG_TIMEOUT: Duration = Duration::from_secs(10);

/// The SIGKILL signal number, as `ExitStatus::signal` reports it.
const SIGKILL_SIGNAL: i32 = 9;

/// How long a child may take to answer `initialize`.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

/// How many attempts one test tolerates before it reports the last outcome.
const SPAWN_ATTEMPTS: usize = 6;

/// How long the harness waits before it retries an attempt.
const RETRY_BACKOFF: Duration = Duration::from_millis(100);

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

/// One direct-driven agent run: the reply, the exit status, and the child's
/// stderr.
struct AgentRun {
    response: String,
    status: ExitStatus,
    stderr: String,
}

impl AgentRun {
    /// Whether the child died without answering anything.
    ///
    /// A SIGKILL with an empty reply is the OS refusing the exec, not the agent
    /// failing: nothing inside the child runs.
    fn refused(&self) -> bool {
        self.response.is_empty() && killed_by_sigkill(&self.status)
    }
}

/// Drive `agent acp` once: send one `initialize`, close stdin, reap the child.
///
/// Closing stdin is the client disconnect: the server sees EOF, publishes
/// Shutdown, drains its hook runners, then exits. Both of the child's pipes are
/// drained on their own threads: a chatty child can never fill one and stall,
/// the reply read carries a deadline, and a failure message carries stderr.
fn drive_agent(env: &TestEnv) -> Result<AgentRun> {
    let mut child = env
        .command()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or("child stdin must be piped")?;
    let stdout = child.stdout.take().ok_or("child stdout must be piped")?;
    let stderr = child.stderr.take().ok_or("child stderr must be piped")?;
    let (reply_tx, reply_rx) = mpsc::channel();
    let stdout_reader = std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = reply_tx.send(line);
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut text);
        text
    });
    writeln!(stdin, "{INITIALIZE_REQUEST}")?;
    stdin.flush()?;
    let response = match reply_rx.recv_timeout(ANSWER_TIMEOUT) {
        Ok(line) => {
            drop(stdin);
            line
        }
        // A held or silent child never answers: stop it and let the empty reply
        // record the refusal.
        Err(_) => {
            let _ = child.kill();
            String::new()
        }
    };
    let status = child.wait()?;
    let _ = stdout_reader.join();
    Ok(AgentRun {
        response,
        status,
        stderr: stderr_reader.join().unwrap_or_default(),
    })
}

/// Drive `agent acp` until an attempt is not refused by the OS.
///
/// An attempt that never answered is retried: the OS can refuse the exec of the
/// freshly linked binary (no reply, a SIGKILL death, or a spawn the layer reports
/// as an absent file), and nothing in the child can prevent it. The last
/// attempt's outcome is what the test reports, so a genuine failure still fails —
/// this retries attempts, it does not soften assertions.
fn drive_serving_agent(env: &TestEnv) -> Result<AgentRun> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        let last = attempts >= SPAWN_ATTEMPTS;
        match drive_agent(env) {
            Ok(run) if run.refused() && !last => {}
            Ok(run) => return Ok(run),
            Err(_) if !last => {}
            Err(err) => return Err(err),
        }
        std::thread::sleep(RETRY_BACKOFF);
    }
}

/// Whether `status` is a SIGKILL death.
#[cfg(unix)]
fn killed_by_sigkill(status: &ExitStatus) -> bool {
    use std::os::unix::process::ExitStatusExt;
    status.signal() == Some(SIGKILL_SIGNAL)
}

/// Whether `status` is a SIGKILL death. Windows reports no signal.
#[cfg(not(unix))]
fn killed_by_sigkill(_status: &ExitStatus) -> bool {
    false
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
    let run = drive_serving_agent(&env)?;

    // -- Check
    assert!(
        run.response.contains("\"protocolVersion\""),
        "the server must answer initialize; got: {:?}; status: {}; stderr: {:?}",
        run.response,
        run.status,
        run.stderr
    );
    assert!(
        run.status.success(),
        "the server must exit cleanly: {}; stderr: {:?}",
        run.status,
        run.stderr
    );
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
