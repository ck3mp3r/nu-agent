use serde_json::Value as JsonValue;
use std::path::Path;
use std::time::Duration;
use tokio::io::AsyncReadExt;

use super::{
    ToolHandlerError,
    builtin_tool::{BuiltinTool, Previewable},
};
use crate::bus::Bus;
use crate::protocol::event::{ToolDisplay, ToolDisplaySection};
use crate::protocol::tool_args::{CallLine, nu_command_from_args, parse_json_usize_field};
use crate::transcript::ir::ContentKind;

const DEFAULT_TIMEOUT_SECONDS: u64 = 120;

#[derive(Debug, serde::Deserialize)]
struct NuArgs {
    command: String,
    #[serde(default)]
    timeout_seconds: Option<u64>,
}

pub struct NuTool;

impl BuiltinTool for NuTool {
    const NAME: &'static str = "nu";

    fn call_line_render(arguments: &str) -> CallLine {
        if nu_command_from_args(arguments).is_none() {
            return CallLine::from_json_summary(arguments);
        }
        // The command renders in the preview block, so the call line carries
        // only the timeout the command is allowed to run for.
        let timeout = parse_json_usize_field(arguments, "timeout_seconds")
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS as usize);
        CallLine {
            summary: format!("⏱ {timeout}s"),
        }
    }

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
        let mut stdout_handle = child.stdout.take().ok_or_else(|| {
            ToolHandlerError::runtime("failed to capture nu stdout pipe".to_string())
        })?;
        let mut stderr_handle = child.stderr.take().ok_or_else(|| {
            ToolHandlerError::runtime("failed to capture nu stderr pipe".to_string())
        })?;

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

impl Previewable for NuTool {
    /// Pre-execution code preview for the permission gate: the nu command
    /// the call is about to run, tagged as `ContentKind::Code` with the
    /// `nu` language. Returns `None` when the command is missing or empty.
    fn preview(args: &JsonValue, _cwd: &Path) -> Option<ToolDisplay> {
        let command = args.get("command")?.as_str()?;
        if command.is_empty() {
            return None;
        }
        let command = command.replace("\r\n", "\n").replace('\r', "\n");
        Some(ToolDisplay {
            title: "nu".to_string(),
            sections: vec![ToolDisplaySection {
                label: "nu".to_string(),
                kind: ContentKind::Code {
                    language: "nu".to_string(),
                },
                content: command,
                stats: None,
            }],
        })
    }
}

#[cfg(test)]
#[path = "nu_test.rs"]
mod tests;
