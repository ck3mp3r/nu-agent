//! The hook subprocess boundary.

use std::future::Future;
use std::path::Path;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

// region:    --- Types

/// A hook subprocess failure.
#[derive(Debug, thiserror::Error)]
pub enum HookProcessError {
    #[error("failed to spawn nu: {0}")]
    Spawn(String),
    #[error("exited with code {exit_code}: {stderr}")]
    Exit { exit_code: i32, stderr: String },
    #[error("timed out after {0:?}")]
    Timeout(Duration),
    #[error("io error: {0}")]
    Io(String),
}

/// Runs one hook script for one event.
///
/// The injected boundary: production uses [`NuProcess`]; unit tests substitute a
/// mock that records the invocation instead of spawning a process.
pub trait HookProcess: Send + Sync {
    fn run(
        &self,
        script: &Path,
        event: &str,
        envelope: &str,
        cwd: &Path,
        timeout: Duration,
    ) -> impl Future<Output = Result<(), HookProcessError>> + Send;
}

/// The production [`HookProcess`]: spawns `nu` and feeds the envelope on stdin.
#[derive(Debug, Clone, Copy, Default)]
pub struct NuProcess;

// endregion: --- Types

// region:    --- NuProcess

impl HookProcess for NuProcess {
    async fn run(
        &self,
        script: &Path,
        event: &str,
        envelope: &str,
        cwd: &Path,
        timeout: Duration,
    ) -> Result<(), HookProcessError> {
        let command = handler_command(script, event);
        let mut child = tokio::process::Command::new("nu")
            .current_dir(cwd)
            // `--stdin` binds the piped envelope to `$in`; without it a `-c`
            // script has no pipeline input at all.
            .arg("--stdin")
            .arg("-c")
            .arg(&command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| HookProcessError::Spawn(e.to_string()))?;

        // Write the envelope and drop the pipe so the handler's `from json`
        // sees end-of-input.
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(envelope.as_bytes())
                .await
                .map_err(|e| HookProcessError::Io(e.to_string()))?;
        }

        let mut stdout_handle = child
            .stdout
            .take()
            .ok_or_else(|| HookProcessError::Io("failed to capture nu stdout".to_string()))?;
        let mut stderr_handle = child
            .stderr
            .take()
            .ok_or_else(|| HookProcessError::Io("failed to capture nu stderr".to_string()))?;

        let (status, stderr) = tokio::select! {
            result = async {
                let mut stdout = String::new();
                let mut stderr = String::new();
                let (stdout_res, stderr_res, status_res) = tokio::join!(
                    stdout_handle.read_to_string(&mut stdout),
                    stderr_handle.read_to_string(&mut stderr),
                    child.wait(),
                );
                let _ = stdout_res;
                let _ = stderr_res;
                (status_res, stderr)
            } => {
                let status = result.0.map_err(|e| HookProcessError::Io(e.to_string()))?;
                (status, result.1)
            }
            _ = tokio::time::sleep(timeout) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(HookProcessError::Timeout(timeout));
            }
        };

        let exit_code = status.code().unwrap_or(-1);
        if exit_code != 0 {
            return Err(HookProcessError::Exit { exit_code, stderr });
        }
        Ok(())
    }
}

/// The `nu` command that sources `script` and calls its `event` handler.
///
/// The envelope arrives on stdin and is bound to `$in` by `nu --stdin`; the
/// handler receives it as a typed record.
fn handler_command(script: &Path, event: &str) -> String {
    let script = script.display().to_string().replace('\'', "''");
    format!("source '{script}'; {event} ($in | from json)")
}

// endregion: --- NuProcess
