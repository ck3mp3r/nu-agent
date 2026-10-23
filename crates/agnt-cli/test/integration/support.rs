//! Shared support for the `agnt-cli` integration tests.
//!
//! Every test builds a [`TestEnv`]: a throwaway XDG tree (config, cache, data,
//! state, home) plus a wiremock server that answers the agent's Ollama
//! completion requests with canned SSE.
//!
//! The XDG variables are process-global. Under `cargo nextest` each test runs
//! in its own process, but `cargo llvm-cov` drives `cargo test`, which runs the
//! whole binary in one process — so every test that builds a `TestEnv` carries
//! `#[serial_test::serial]` and the environment is restored on drop.

use std::ffi::OsString;
use std::io::IsTerminal;
use std::path::PathBuf;

use agnt_testkit::MockServer;
use clap::Parser;

use super::*;

pub type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- TestEnv

/// A throwaway XDG tree plus a mock LLM server.
pub struct TestEnv {
    /// Restores the process environment when the value drops.
    _restore: EnvRestore,
    /// Removes the temp directory when the value drops.
    _temp: tempfile::TempDir,
    config_home: PathBuf,
    cache_home: PathBuf,
    data_home: PathBuf,
    state_home: PathBuf,
    llm: MockServer,
}

impl TestEnv {
    /// Create the XDG tree, point the process environment at it, and start the
    /// mock LLM server.
    pub async fn new() -> Result<Self> {
        // `main()` installs the ring crypto provider before any client is
        // built. The test harness must do the same, or reqwest panics when the
        // MCP OAuth path builds a TLS client. A second install is a no-op.
        let _ = rustls::crypto::ring::default_provider().install_default();

        let temp = tempfile::tempdir()?;
        let config_home = temp.path().join("config");
        let cache_home = temp.path().join("cache");
        let data_home = temp.path().join("data");
        let state_home = temp.path().join("state");
        let home = temp.path().join("home");
        for dir in [&config_home, &cache_home, &data_home, &state_home, &home] {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::create_dir_all(config_home.join("nu-agent"))?;

        let mut restore = EnvRestore::apply(&[
            ("XDG_CONFIG_HOME", config_home.clone()),
            ("XDG_CACHE_HOME", cache_home.clone()),
            ("XDG_DATA_HOME", data_home.clone()),
            ("XDG_STATE_HOME", state_home.clone()),
            ("HOME", home),
            // The file backend writes into the tempdir data home. It probes no
            // keychain and reaches no network, so the vault stays hermetic
            // while still supporting the credential write paths.
            ("AGENT_VAULT_BACKEND", PathBuf::from("file")),
        ]);
        // Session commands read the store type from the process environment.
        // An ambient value would point them at a store the fixtures never
        // seeded. A test that wants the variable sets it after this call.
        restore.remove(&["AGENT_SESSION_STORE_TYPE"]);

        let llm = agnt_testkit::start_server().await;

        Ok(Self {
            _restore: restore,
            _temp: temp,
            config_home,
            cache_home,
            data_home,
            state_home,
            llm,
        })
    }

    /// The mock LLM server.
    pub fn llm(&self) -> &MockServer {
        &self.llm
    }

    /// The `config.toml` path the CLI resolves from the environment.
    pub fn config_path(&self) -> PathBuf {
        self.config_home.join("nu-agent").join("config.toml")
    }

    /// Write `contents` to the resolved `config.toml`.
    pub fn write_config(&self, contents: &str) -> Result<()> {
        std::fs::write(self.config_path(), contents)?;
        Ok(())
    }

    /// Write a `config.toml` whose `default` role points at the mock LLM
    /// through the `ollama` provider.
    pub fn write_default_config(&self) -> Result<()> {
        self.write_config(&format!(
            "[models.default]\nmodel = \"ollama/llama3\"\n\n[providers.ollama]\nbase_url = \"{}\"\n",
            self.llm.uri()
        ))
    }

    /// A path inside the XDG cache home.
    pub fn cache_home_path(&self, relative: &str) -> PathBuf {
        self.cache_home.join(relative)
    }

    /// A path inside the XDG data home.
    pub fn data_home_path(&self, relative: &str) -> PathBuf {
        self.data_home.join(relative)
    }

    /// A path inside the XDG state home.
    pub fn state_home_path(&self, relative: &str) -> PathBuf {
        self.state_home.join(relative)
    }
}

// endregion: --- TestEnv

// region:    --- EnvRestore

/// Restores the process environment when dropped.
pub struct EnvRestore {
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl EnvRestore {
    /// Save the current value of every key, then set each to its new value.
    fn apply(vars: &[(&'static str, PathBuf)]) -> Self {
        let mut saved = Vec::with_capacity(vars.len());
        for (key, value) in vars {
            saved.push((*key, std::env::var_os(key)));
            // SAFETY: every test that mutates the environment is `#[serial]`,
            // so no other test reads it while these are set.
            unsafe { std::env::set_var(key, value) };
        }
        Self { saved }
    }

    /// Save the current value of every key, then remove each from the
    /// environment. Drop restores the saved values.
    fn remove(&mut self, keys: &[&'static str]) {
        for key in keys {
            self.saved.push((*key, std::env::var_os(key)));
            // SAFETY: every test that mutates the environment is `#[serial]`,
            // so no other test reads it while these are removed.
            unsafe { std::env::remove_var(key) };
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..) {
            match value {
                // SAFETY: see `EnvRestore::apply`.
                Some(value) => unsafe { std::env::set_var(key, value) },
                // SAFETY: see `EnvRestore::apply`.
                None => unsafe { std::env::remove_var(key) },
            }
        }
    }
}

/// Set `vars` for the lifetime of the returned guard.
pub fn set_env(vars: &[(&'static str, &str)]) -> EnvRestore {
    let owned: Vec<(&'static str, PathBuf)> = vars
        .iter()
        .map(|(key, value)| (*key, PathBuf::from(value)))
        .collect();
    EnvRestore::apply(&owned)
}

/// Remove `keys` for the lifetime of the returned guard.
pub fn unset_env(keys: &[&'static str]) -> EnvRestore {
    let mut restore = EnvRestore {
        saved: Vec::with_capacity(keys.len()),
    };
    restore.remove(keys);
    restore
}

// endregion: --- EnvRestore

// region:    --- Terminal state

/// Whether the process terminal state selects the interactive TUI mode.
///
/// `run_agent` without a prompt and `run_a2a` derive their mode from the
/// terminal state. When both stdin and stderr are terminals the CLI takes over
/// the terminal, so those tests skip instead of driving a real TUI.
pub fn tui_mode_selected() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

// endregion: --- Terminal state

// region:    --- CLI helpers

/// Parse `argv` (without the program name) into a [`Cli`].
///
/// `try_parse_from` is used instead of `parse_from` so a bad fixture surfaces
/// as a test error rather than a process exit.
pub fn parse_cli(argv: &[&str]) -> Result<Cli> {
    let mut full = vec!["agent"];
    full.extend_from_slice(argv);
    Ok(Cli::try_parse_from(full)?)
}

/// Parse `argv` and return the `session` subcommand.
pub fn parse_session_command(argv: &[&str]) -> Result<(AgentArgs, SessionCommand)> {
    let cli = parse_cli(argv)?;
    let Some(Command::Session(command)) = cli.command else {
        return Err(format!("expected a session subcommand in {argv:?}").into());
    };
    Ok((cli.args, command))
}

/// Parse `argv` and return the `mcp` subcommand.
pub fn parse_mcp_command(argv: &[&str]) -> Result<McpCommand> {
    let cli = parse_cli(argv)?;
    let Some(Command::Mcp(command)) = cli.command else {
        return Err(format!("expected an mcp subcommand in {argv:?}").into());
    };
    Ok(command)
}

/// Parse `argv` and return the `provider` subcommand.
pub fn parse_provider_command(argv: &[&str]) -> Result<ProviderCommand> {
    let cli = parse_cli(argv)?;
    let Some(Command::Provider(command)) = cli.command else {
        return Err(format!("expected a provider subcommand in {argv:?}").into());
    };
    Ok(command)
}

/// Parse `argv` and return the `models` subcommand.
pub fn parse_models_command(argv: &[&str]) -> Result<ModelsCommand> {
    let cli = parse_cli(argv)?;
    let Some(Command::Models(command)) = cli.command else {
        return Err(format!("expected a models subcommand in {argv:?}").into());
    };
    Ok(command)
}

/// Parse `argv` and return the `config` subcommand.
pub fn parse_config_command(argv: &[&str]) -> Result<ConfigCommand> {
    let cli = parse_cli(argv)?;
    let Some(Command::Config(command)) = cli.command else {
        return Err(format!("expected a config subcommand in {argv:?}").into());
    };
    Ok(command)
}

/// Parse `argv` and return the `a2a` arguments.
pub fn parse_a2a_args(argv: &[&str]) -> Result<(AgentArgs, A2aArgs)> {
    let cli = parse_cli(argv)?;
    let Some(Command::A2a(a2a)) = cli.command else {
        return Err(format!("expected the a2a subcommand in {argv:?}").into());
    };
    Ok((cli.args, a2a))
}

// endregion: --- CLI helpers

// region:    --- Fixtures

/// A `models.json` cache holding one provider with two models.
pub const MODELS_CACHE: &str = r#"
{
  "ollama": {
    "id": "ollama",
    "name": "Ollama",
    "api": "http://localhost:11434",
    "models": {
      "llama3": {
        "id": "llama3",
        "name": "Llama 3",
        "tool_call": true,
        "limit": { "context": 8192, "output": 2048 },
        "cost": { "input": 0.5, "output": 2.0 }
      },
      "qwen3": {
        "id": "qwen3",
        "name": "Qwen 3",
        "tool_call": false,
        "limit": { "context": 32768, "output": 4096 }
      }
    }
  }
}
"#;

/// Write [`MODELS_CACHE`] to the resolved models cache path.
pub fn write_models_cache(env: &TestEnv) -> Result<PathBuf> {
    let path = env.data_home_path("nu-agent/models.json");
    let parent = path.parent().ok_or("models cache path has no parent")?;
    std::fs::create_dir_all(parent)?;
    std::fs::write(&path, MODELS_CACHE)?;
    Ok(path)
}

// endregion: --- Fixtures
