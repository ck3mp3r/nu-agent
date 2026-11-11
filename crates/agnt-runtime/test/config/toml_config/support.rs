//! Shared fixtures for the `config.toml` loading tests.

use crate::config::cmd_refs::{CmdRunError, CmdRunner};
use crate::utils::env_map::EnvMap;
use tempfile::TempDir;

pub(crate) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Fixtures

/// Helper to run a test with a controlled XDG_CONFIG_HOME pointing to a temp dir.
pub(crate) fn with_xdg_config_home<F>(test: F) -> Result<()>
where
    F: FnOnce(&TempDir) -> Result<()>,
{
    let dir = TempDir::new().map_err(|e| format!("failed to create temp dir: {e}"))?;
    test(&dir)
}

/// Build an [`EnvMap`] pointing `XDG_CONFIG_HOME` at `dir`.
pub(crate) fn env_for(dir: &TempDir) -> EnvMap {
    EnvMap::from([(
        "XDG_CONFIG_HOME".to_string(),
        dir.path().to_string_lossy().to_string(),
    )])
}

/// Answers `gh auth token` and `op read 'OpenAI API Key'`; fails everything else.
pub(crate) struct StubRunner;

impl CmdRunner for StubRunner {
    fn run(&self, program: &str, args: &[&str]) -> core::result::Result<String, CmdRunError> {
        match (program, args) {
            ("gh", ["auth", "token"]) => Ok("ghp_from_stub\n".to_string()),
            ("op", ["read", "OpenAI API Key"]) => Ok("sk-op\n".to_string()),
            _ => Err(CmdRunError::Io(std::io::Error::other(format!(
                "unexpected command: {program} {args:?}"
            )))),
        }
    }
}

/// Fails every command with a non-zero exit.
pub(crate) struct FailingRunner;

impl CmdRunner for FailingRunner {
    fn run(&self, _program: &str, _args: &[&str]) -> core::result::Result<String, CmdRunError> {
        Err(CmdRunError::Io(std::io::Error::other("stub failure")))
    }
}

/// Write `contents` to `<dir>/nu-agent/config.toml` and return the path.
pub(crate) fn write_config(dir: &TempDir, contents: &str) -> Result<std::path::PathBuf> {
    let config_dir = dir.path().join("nu-agent");
    std::fs::create_dir_all(&config_dir)
        .map_err(|e| format!("failed to create config dir: {e}"))?;
    let path = config_dir.join("config.toml");
    std::fs::write(&path, contents).map_err(|e| format!("failed to write config: {e}"))?;
    Ok(path)
}

// endregion: --- Fixtures
