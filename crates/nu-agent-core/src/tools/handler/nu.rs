use serde_json::Value as JsonValue;
use std::path::Path;
use std::time::{Duration, Instant};

use super::ToolHandlerError;

const DEFAULT_TIMEOUT_SECONDS: u64 = 300;
const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, serde::Deserialize)]
struct NuArgs {
    command: String,
    #[serde(default)]
    timeout_seconds: Option<u64>,
}

/// Parse tool arguments into a typed struct, mapping serde errors to validation errors.
fn parse_args(arguments: &JsonValue) -> Result<NuArgs, ToolHandlerError> {
    serde_json::from_value(arguments.clone())
        .map_err(|e| ToolHandlerError::validation(format!("Invalid nu arguments: {e}")))
}

/// Wait for a child process to exit, killing it if it exceeds the timeout.
///
/// Returns the exit status on success, or a runtime error if the process timed out.
fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<std::process::ExitStatus, ToolHandlerError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| ToolHandlerError::runtime(format!("Failed to wait for nu process: {e}")))?
        {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ToolHandlerError::runtime(format!(
                "command timed out after {} seconds",
                timeout.as_secs()
            )));
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Build the JSON result returned to the LLM from captured output and exit status.
fn build_result(stdout: String, stderr: String, exit_code: i32) -> JsonValue {
    serde_json::json!({
        "stdout": stdout,
        "stderr": stderr,
        "exit_code": exit_code,
    })
}

/// Dispatch a `nu` tool call: execute a Nushell command in the given working directory.
pub fn dispatch_nu_tool(
    tool_name: &str,
    arguments: &JsonValue,
    cwd: &Path,
) -> Result<Option<JsonValue>, ToolHandlerError> {
    if tool_name != "nu" {
        return Ok(None);
    }

    let args = parse_args(arguments)?;
    let timeout = Duration::from_secs(args.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS));

    let mut child = std::process::Command::new("nu")
        .current_dir(cwd)
        .arg("-c")
        .arg(&args.command)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| ToolHandlerError::runtime(format!("Failed to spawn nu: {e}")))?;

    // Take the pipe handles before polling and drain them on reader threads.
    // Reading stdout/stderr only after the child exits can deadlock: if output
    // exceeds the OS pipe buffer, the child blocks on write and never exits.
    let mut stdout_handle = child.stdout.take().expect("stdout piped");
    let mut stderr_handle = child.stderr.take().expect("stderr piped");
    let stdout_thread = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = String::new();
        let _ = stdout_handle.read_to_string(&mut buf);
        buf
    });
    let stderr_thread = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = String::new();
        let _ = stderr_handle.read_to_string(&mut buf);
        buf
    });

    let status = wait_with_timeout(&mut child, timeout)?;

    // join() unwrap_or_default is panic-recovery fallback for a poisoned thread.
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();

    Ok(Some(build_result(
        stdout,
        stderr,
        status.code().unwrap_or(-1),
    )))
}

#[cfg(test)]
#[path = "nu_test.rs"]
mod tests;
