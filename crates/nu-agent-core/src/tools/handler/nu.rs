use serde_json::Value as JsonValue;
use std::path::Path;
use std::time::Duration;
use tokio::io::AsyncReadExt;

use super::{ToolHandlerError, builtin_tool::BuiltinTool};
use crate::bus::Bus;

const DEFAULT_TIMEOUT_SECONDS: u64 = 300;

#[derive(Debug, serde::Deserialize)]
struct NuArgs {
    command: String,
    #[serde(default)]
    timeout_seconds: Option<u64>,
}

pub struct NuTool;

impl BuiltinTool for NuTool {
    const NAME: &'static str = "nu";

    async fn execute(
        args: &JsonValue,
        cwd: &Path,
        bus: &Bus,
    ) -> Result<JsonValue, ToolHandlerError> {
        let nu_args: NuArgs = serde_json::from_value(args.clone())
            .map_err(|e| ToolHandlerError::validation(format!("Invalid nu arguments: {e}")))?;

        let timeout =
            Duration::from_secs(nu_args.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS));

        let mut child = tokio::process::Command::new("nu")
            .current_dir(cwd)
            .arg("-c")
            .arg(&nu_args.command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ToolHandlerError::runtime(format!("Failed to spawn nu: {e}")))?;

        // Take pipe handles before select!
        let mut stdout_handle = child.stdout.take().expect("stdout piped");
        let mut stderr_handle = child.stderr.take().expect("stderr piped");

        let mut cancel_rx = bus.cancel().subscribe();

        let (status, stdout, stderr) = tokio::select! {
            // Primary arm: read pipes + wait concurrently
            result = async {
                let mut stdout = String::new();
                let mut stderr = String::new();

                // tokio::join! polls all three concurrently
                let (stdout_res, stderr_res, status_res) = tokio::join!(
                    stdout_handle.read_to_string(&mut stdout),
                    stderr_handle.read_to_string(&mut stderr),
                    child.wait(),
                );
                let _ = stdout_res;
                let _ = stderr_res;
                (status_res, stdout, stderr)
            } => {
                let status = result.0.map_err(|e| ToolHandlerError::runtime(format!("Failed to wait for nu process: {e}")))?;
                (status, result.1, result.2)
            }
            // Timeout: kill child, return error
            _ = tokio::time::sleep(timeout) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(ToolHandlerError::runtime(format!(
                    "command timed out after {} seconds",
                    timeout.as_secs()
                )));
            }
            // Cancellation: kill child, return error
            Ok(_) = cancel_rx.recv() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(ToolHandlerError::runtime("nu command cancelled by user"));
            }
        };

        Ok(serde_json::json!({
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": status.code().unwrap_or(-1),
        }))
    }
}

#[cfg(test)]
#[path = "nu_test.rs"]
mod tests;
