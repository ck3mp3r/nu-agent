use serde_json::Value as JsonValue;
use std::path::Path;
use std::process::ExitStatus;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio::time::Instant;

use super::{ToolHandlerError, builtin_tool::BuiltinTool};
use crate::bus::{CancelRx, CancelTx};

const DEFAULT_TIMEOUT_SECONDS: u64 = 120;

#[derive(Debug, serde::Deserialize)]
struct NuArgs {
    command: String,
    #[serde(default)]
    timeout_seconds: Option<u64>,
}

pub struct NuTool;

impl NuTool {}

impl BuiltinTool for NuTool {
    const NAME: &'static str = "nu";

    async fn execute(
        args: &JsonValue,
        cwd: &Path,
        cancel: &CancelTx,
    ) -> Result<JsonValue, ToolHandlerError> {
        let nu_args: NuArgs = serde_json::from_value(args.clone())
            .map_err(|e| ToolHandlerError::validation(format!("Invalid nu arguments: {e}")))?;

        let timeout =
            Duration::from_secs(nu_args.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS));
        // The wall clock covers the whole call, capture drains included: the
        // `nu -c` wrapper can exit while a process it spawned keeps the
        // inherited pipes open, so the wrapper's exit is not the call's end.
        let deadline = Instant::now() + timeout;

        let mut command = Command::new("nu");
        command
            .current_dir(cwd)
            .arg("-c")
            .arg(&nu_args.command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        // The wrapper leads its own process group, so the kill below reaches
        // every process the command spawned, not just the `nu` wrapper.
        #[cfg(unix)]
        command.process_group(0);

        let mut child = command
            .spawn()
            .map_err(|e| ToolHandlerError::runtime(format!("Failed to spawn nu: {e}")))?;
        let process_group = child.id();

        // Take the pipe handles before the child moves into the guard task.
        let stdout_handle = child.stdout.take().ok_or_else(|| {
            ToolHandlerError::runtime("failed to capture nu stdout pipe".to_string())
        })?;
        let stderr_handle = child.stderr.take().ok_or_else(|| {
            ToolHandlerError::runtime("failed to capture nu stderr pipe".to_string())
        })?;

        // The guard owns the child, the pipes, and the cancel subscription, so
        // the kill never depends on this future surviving: the turn's tool
        // proxy drops it the moment its own cancel arm wins.
        let guard = tokio::spawn(guard_call(
            child,
            stdout_handle,
            stderr_handle,
            process_group,
            deadline,
            cancel.subscribe(),
        ));

        let (status, stdout, stderr) = match guard
            .await
            .map_err(|e| ToolHandlerError::runtime(format!("nu process guard failed: {e}")))?
            .map_err(|e| ToolHandlerError::runtime(format!("Failed to wait for nu process: {e}")))?
        {
            GuardOutcome::Exited {
                status,
                stdout,
                stderr,
            } => (status, stdout, stderr),
            // The wall clock stopped the call before it produced a result, so
            // the kind is not-run (the message is the LLM-facing text either way).
            GuardOutcome::TimedOut => {
                return Err(ToolHandlerError::not_run(format!(
                    "command timed out after {} seconds",
                    timeout.as_secs()
                )));
            }
            GuardOutcome::Cancelled => {
                return Err(ToolHandlerError::not_run("nu command cancelled by user"));
            }
        };

        let exit_code = status.code().unwrap_or(-1);
        if exit_code != 0 {
            // Non-zero exits are failures: the producer carries the state
            // structurally (kind + details) instead of a success-shaped
            // payload the hook has to re-derive from output text.
            return Err(
                ToolHandlerError::runtime(format!("command exited with code {exit_code}"))
                    .with_details(serde_json::json!({
                        "stdout": stdout,
                        "stderr": stderr,
                        "exit_code": exit_code,
                    })),
            );
        }

        Ok(serde_json::json!({
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": exit_code,
        }))
    }
}

// region:    --- Support

/// How a spawned nu call finished. The guard task owns the child and the
/// pipes, so its outcome is the single source of truth for the exit status,
/// both captured streams, and both wall-clock stops.
enum GuardOutcome {
    Exited {
        status: ExitStatus,
        stdout: String,
        stderr: String,
    },
    TimedOut,
    Cancelled,
}

/// Run one nu call under the wall clock, killing the whole process tree on
/// cancel and timeout.
///
/// The call ends when both pipes reach EOF and the wrapper is reaped — never
/// when the wrapper alone exits: `nu -c '<daemon> &'` returns at once while
/// the daemon keeps the inherited pipes open. Every stop is bounded by
/// `deadline`, which the caller sets at call start.
///
/// The wrapper stays unreaped until the call actually ends, so its pid — the
/// process-group id the kill signals — cannot be recycled while a stop arm may
/// still fire.
///
/// The guard runs as its own task: a caller that drops the tool future (the
/// turn's tool proxy does, when its own cancel arm wins the race) does not
/// take the kill with it.
async fn guard_call(
    mut child: Child,
    stdout: ChildStdout,
    stderr: ChildStderr,
    process_group: Option<u32>,
    deadline: Instant,
    mut cancel_rx: CancelRx,
) -> std::io::Result<GuardOutcome> {
    let captured = tokio::select! {
        captured = async { tokio::join!(capture(stdout), capture(stderr)) } => captured,
        _ = tokio::time::sleep_until(deadline) => {
            return stop_call(&mut child, process_group, GuardOutcome::TimedOut).await;
        }
        Ok(_) = cancel_rx.recv() => {
            return stop_call(&mut child, process_group, GuardOutcome::Cancelled).await;
        }
    };

    // Both pipes closed, so only the exit status is left. Still under the same
    // clock: a command may close its outputs and keep running.
    tokio::select! {
        status = child.wait() => Ok(GuardOutcome::Exited {
            status: status?,
            stdout: captured.0,
            stderr: captured.1,
        }),
        _ = tokio::time::sleep_until(deadline) => {
            stop_call(&mut child, process_group, GuardOutcome::TimedOut).await
        }
        Ok(_) = cancel_rx.recv() => {
            stop_call(&mut child, process_group, GuardOutcome::Cancelled).await
        }
    }
}

/// SIGKILL the whole tree, reap the wrapper, and report the stop.
async fn stop_call(
    child: &mut Child,
    process_group: Option<u32>,
    outcome: GuardOutcome,
) -> std::io::Result<GuardOutcome> {
    kill_process_tree(child, process_group).await;
    Ok(outcome)
}

/// SIGKILL the whole tree, then reap the wrapper.
async fn kill_process_tree(child: &mut Child, process_group: Option<u32>) {
    kill_process_group(process_group);
    let _ = child.start_kill();
    let _ = child.wait().await;
}

/// Signal every process in the group the wrapper leads. The agent process is
/// not a member of that group, so the negative pid can never reach it.
#[cfg(unix)]
fn kill_process_group(process_group: Option<u32>) {
    let Some(process_group) = process_group else {
        return;
    };
    // SAFETY: `kill` reads only its arguments. The group id is the pid of the
    // unreaped wrapper, which `process_group(0)` made the group leader.
    let _ = unsafe { libc::kill(-(process_group as libc::pid_t), libc::SIGKILL) };
}

/// Without process groups the wrapper kill is all that is available.
#[cfg(not(unix))]
fn kill_process_group(_process_group: Option<u32>) {}

/// Drain one pipe to EOF, alongside the other pipe, so neither stream can
/// block the other.
async fn capture<R: AsyncRead + Unpin>(mut pipe: R) -> String {
    let mut captured = String::new();
    let _ = pipe.read_to_string(&mut captured).await;
    captured
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/tools/handler/nu.rs"]
mod tests;

#[cfg(all(test, feature = "integration"))]
#[path = "../../../test/tools/handler/nu_cancel.rs"]
mod nu_cancel_test;

// endregion: --- Tests
