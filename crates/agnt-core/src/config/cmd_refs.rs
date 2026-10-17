//! Eager `cmd:` reference resolution for config.toml values.
//!
//! A string value that starts with `cmd:` is replaced by the trimmed stdout of
//! the command that follows the prefix. The walk runs once at config load, so
//! every string value in the file — provider API keys, MCP environment
//! variables, MCP headers, agent settings — resolves through one code path.

use std::io::Read;
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

// region:    --- Constants

/// Prefix that marks a config value as a command reference.
pub const CMD_PREFIX: &str = "cmd:";

/// Default timeout for a `cmd:` helper process.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// How often the runner polls the child process for exit.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

// endregion: --- Constants

// region:    --- Types

/// Failure detail from a [`CmdRunner`].
#[derive(Debug, thiserror::Error)]
pub enum CmdRunError {
    /// The process could not be spawned, or the runner could not observe it.
    #[error("failed to run: {0}")]
    Io(#[source] std::io::Error),
    /// The process exited with a non-zero status.
    #[error("exited with status {code}{stderr}")]
    Exit {
        /// Process exit code; `-1` means the process was killed by a signal.
        code: i32,
        /// Captured stderr, rendered only when it is non-empty.
        stderr: StderrSuffix,
    },
    /// The process did not finish within the runner timeout.
    #[error("timed out after {0:?}")]
    Timeout(Duration),
}

/// Renders captured stderr as a message suffix, or nothing when it is empty.
#[derive(Debug)]
pub struct StderrSuffix(String);

impl StderrSuffix {
    /// Wrap raw stderr text.
    pub fn new(stderr: impl Into<String>) -> Self {
        Self(stderr.into())
    }
}

impl std::fmt::Display for StderrSuffix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let trimmed = self.0.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        write!(f, ", stderr: {trimmed}")
    }
}

/// Failure while resolving a `cmd:` reference.
#[derive(Debug, thiserror::Error)]
pub enum CmdRefError {
    /// The command failed to run.
    #[error("config value `{key_path}`: `cmd:` reference `{command}` {source}")]
    Run {
        /// Dot-joined key path of the offending value.
        key_path: String,
        /// The command text after the `cmd:` prefix.
        command: String,
        /// The runner failure.
        #[source]
        source: CmdRunError,
    },
    /// The command produced no output after trimming.
    #[error("config value `{key_path}`: `cmd:` reference `{command}` produced empty output")]
    Empty {
        /// Dot-joined key path of the offending value.
        key_path: String,
        /// The command text after the `cmd:` prefix.
        command: String,
    },
    /// The reference carries no command after the prefix.
    #[error("config value `{key_path}`: `cmd:` reference `{command}` has no command")]
    EmptyCommand {
        /// Dot-joined key path of the offending value.
        key_path: String,
        /// The raw value, including the `cmd:` prefix.
        command: String,
    },
    /// The command text could not be split into arguments.
    #[error("config value `{key_path}`: `cmd:` reference `{command}` has unbalanced quotes")]
    Split {
        /// Dot-joined key path of the offending value.
        key_path: String,
        /// The command text after the `cmd:` prefix.
        command: String,
    },
}

/// Runs a `cmd:` helper and returns its raw stdout.
pub trait CmdRunner {
    /// Run `program` with `args` and return its raw stdout.
    ///
    /// The caller trims trailing newlines and rejects empty output.
    fn run(&self, program: &str, args: &[&str]) -> Result<String, CmdRunError>;
}

/// Production [`CmdRunner`] that spawns real child processes.
#[derive(Debug, Clone)]
pub struct ProcessCmdRunner {
    timeout: Duration,
}

// endregion: --- Types

// region:    --- Functions

/// Replace every `cmd:` string value in `value` with the command's stdout.
///
/// Table values and array elements are visited; table keys are not. A resolved
/// value is never re-resolved, so an output that starts with `cmd:` stays
/// literal.
pub fn resolve_cmd_refs(
    value: &mut toml::Value,
    runner: &dyn CmdRunner,
) -> Result<(), CmdRefError> {
    walk(value, "", runner)
}

// endregion: --- Functions

// region:    --- ProcessCmdRunner

impl Default for ProcessCmdRunner {
    fn default() -> Self {
        Self::new(DEFAULT_TIMEOUT)
    }
}

impl ProcessCmdRunner {
    /// Construct a runner with an explicit timeout.
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl CmdRunner for ProcessCmdRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<String, CmdRunError> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(CmdRunError::Io)?;

        let Some(stdout) = child.stdout.take() else {
            return Err(CmdRunError::Io(std::io::Error::other(
                "child stdout pipe is unavailable",
            )));
        };
        let Some(stderr) = child.stderr.take() else {
            return Err(CmdRunError::Io(std::io::Error::other(
                "child stderr pipe is unavailable",
            )));
        };

        let stdout_reader = std::thread::spawn(move || read_pipe(stdout));
        let stderr_reader = std::thread::spawn(move || read_pipe(stderr));

        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(error) => return Err(CmdRunError::Io(error)),
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CmdRunError::Timeout(self.timeout));
            }
            std::thread::sleep(POLL_INTERVAL);
        };

        let stdout_text = join_reader(stdout_reader)?;
        let stderr_text = join_reader(stderr_reader)?;

        if !status.success() {
            return Err(CmdRunError::Exit {
                code: status.code().unwrap_or(-1),
                stderr: StderrSuffix::new(stderr_text),
            });
        }
        Ok(stdout_text)
    }
}

// endregion: --- ProcessCmdRunner

// region:    --- Support

fn walk(
    value: &mut toml::Value,
    key_path: &str,
    runner: &dyn CmdRunner,
) -> Result<(), CmdRefError> {
    match value {
        toml::Value::String(text) => {
            if let Some(resolved) = resolve_value(text, key_path, runner)? {
                *text = resolved;
            }
        }
        toml::Value::Table(table) => {
            for (key, child) in table.iter_mut() {
                let child_path = join_key(key_path, key);
                walk(child, &child_path, runner)?;
            }
        }
        toml::Value::Array(items) => {
            for (index, child) in items.iter_mut().enumerate() {
                let child_path = format!("{key_path}[{index}]");
                walk(child, &child_path, runner)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn join_key(key_path: &str, key: &str) -> String {
    if key_path.is_empty() {
        key.to_string()
    } else {
        format!("{key_path}.{key}")
    }
}

fn resolve_value(
    text: &str,
    key_path: &str,
    runner: &dyn CmdRunner,
) -> Result<Option<String>, CmdRefError> {
    let Some(reference) = text.strip_prefix(CMD_PREFIX) else {
        return Ok(None);
    };
    let command = reference.trim();
    let Some(parts) = shlex::split(command) else {
        return Err(CmdRefError::Split {
            key_path: key_path.to_string(),
            command: command.to_string(),
        });
    };
    let Some((program, args)) = parts.split_first() else {
        return Err(CmdRefError::EmptyCommand {
            key_path: key_path.to_string(),
            command: text.to_string(),
        });
    };
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let stdout = runner
        .run(program, &args)
        .map_err(|source| CmdRefError::Run {
            key_path: key_path.to_string(),
            command: command.to_string(),
            source,
        })?;
    let trimmed = stdout.trim_end_matches(['\n', '\r']);
    if trimmed.is_empty() {
        return Err(CmdRefError::Empty {
            key_path: key_path.to_string(),
            command: command.to_string(),
        });
    }
    Ok(Some(trimmed.to_string()))
}

fn read_pipe(mut pipe: impl Read) -> std::io::Result<String> {
    let mut buffer = Vec::new();
    pipe.read_to_end(&mut buffer)?;
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

fn join_reader(handle: JoinHandle<std::io::Result<String>>) -> Result<String, CmdRunError> {
    match handle.join() {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(error)) => Err(CmdRunError::Io(error)),
        Err(_) => Err(CmdRunError::Io(std::io::Error::other(
            "pipe reader thread panicked",
        ))),
    }
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/config/cmd_refs.rs"]
mod cmd_refs_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/config/cmd_refs_process.rs"]
mod cmd_refs_process_test;

// endregion: --- Tests
