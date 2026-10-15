//! `agent` — command-line entry point for the agent runtime.

// region:    --- Modules

pub mod dispatch;
pub mod error;
pub mod mode;

use std::io::IsTerminal;

use clap::{ArgAction, Args, Parser, Subcommand};

// endregion: --- Modules

// region:    --- Types

/// Version string printed by `agent --version`.
///
/// Composed at build time by `build.rs`: the cargo package version plus an
/// optional ` (<short sha>)` suffix.
const VERSION_MESSAGE: &str = env!("AGENT_VERSION_MESSAGE");

/// Command-line interface for the `agent` agent runtime.
#[derive(Debug, Parser)]
#[command(
    name = "agent",
    version = VERSION_MESSAGE,
    about = "Run the agent runtime in TTY/TUI, ACP, or A2A mode",
    long_about = None
)]
pub struct Cli {
    /// Runtime mode. Omit to start the TTY/TUI mode.
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Prompt for a single turn. When omitted, stdin is read if it is not a
    /// terminal; otherwise the interactive TUI starts.
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,

    #[command(flatten)]
    pub ui: UiArgs,

    #[command(flatten)]
    pub args: AgentArgs,
}

/// Runtime mode selected on the command line.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the Agent Client Protocol server over stdio.
    Acp,
    /// Run the A2A agent server.
    A2a(A2aArgs),
    /// Manage the agent configuration file.
    #[command(subcommand)]
    Config(ConfigCommand),
    /// Manage the local models cache.
    #[command(subcommand)]
    Models(ModelsCommand),
    /// Manage LLM provider credentials.
    #[command(subcommand)]
    Provider(ProviderCommand),
    /// Manage MCP server credentials.
    #[command(subcommand)]
    Mcp(McpCommand),
    /// Manage stored sessions.
    #[command(subcommand)]
    Session(SessionCommand),
}

/// `agent config` subcommands.
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Generate a starter config.toml from current environment variables.
    Init(ConfigInitArgs),
}

/// Arguments for `agent config init`.
#[derive(Debug, Args)]
pub struct ConfigInitArgs {
    /// Overwrite an existing config.toml.
    #[arg(long)]
    pub force: bool,
}

/// `agent models` subcommands.
#[derive(Debug, Subcommand)]
pub enum ModelsCommand {
    /// Fetch the latest model specs from models.dev and update the local cache.
    Sync,
    /// List available models from the local cache.
    List(ModelsListArgs),
}

/// Arguments for `agent models list`.
#[derive(Debug, Args)]
pub struct ModelsListArgs {
    /// Filter by provider name.
    #[arg(long)]
    pub provider: Option<String>,
}

/// `agent provider` subcommands.
#[derive(Debug, Subcommand)]
pub enum ProviderCommand {
    /// Manage provider authentication.
    #[command(subcommand)]
    Auth(ProviderAuthCommand),
}

/// `agent provider auth` subcommands.
#[derive(Debug, Subcommand)]
pub enum ProviderAuthCommand {
    /// Authenticate with an LLM provider.
    Login(ProviderLoginArgs),
    /// Clear stored credentials for an LLM provider.
    Logout(ProviderLogoutArgs),
    /// Show stored credentials for all LLM providers.
    Status,
}

/// Arguments for `agent provider auth login`.
#[derive(Debug, Args)]
pub struct ProviderLoginArgs {
    /// Provider name (e.g. 'openai', 'anthropic', 'github-copilot').
    pub name: String,

    /// API key to store (alternatively, pipe it via stdin).
    #[arg(long)]
    pub api_key: Option<String>,
}

/// Arguments for `agent provider auth logout`.
#[derive(Debug, Args)]
pub struct ProviderLogoutArgs {
    /// Provider name to clear credentials for.
    pub name: String,
}

/// `agent mcp` subcommands.
#[derive(Debug, Subcommand)]
pub enum McpCommand {
    /// Manage MCP server authentication.
    #[command(subcommand)]
    Auth(McpAuthCommand),
}

/// `agent mcp auth` subcommands.
#[derive(Debug, Subcommand)]
pub enum McpAuthCommand {
    /// Authenticate with an MCP server via OAuth.
    Login(McpLoginArgs),
    /// Clear stored OAuth credentials for an MCP server.
    Logout(McpLogoutArgs),
    /// Show authentication status for all configured MCP servers.
    Status,
}

/// Arguments for `agent mcp auth login`.
#[derive(Debug, Args)]
pub struct McpLoginArgs {
    /// MCP server name to authenticate with.
    pub server: String,
}

/// Arguments for `agent mcp auth logout`.
#[derive(Debug, Args)]
pub struct McpLogoutArgs {
    /// MCP server name to clear credentials for.
    pub server: String,
}

/// `agent session` subcommands.
#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// List all sessions with their statistics.
    List,
    /// Delete a session.
    Clear(SessionIdArgs),
    /// Display full details of a specific session.
    Inspect(SessionIdArgs),
}

/// Arguments for session commands that take a session ID.
#[derive(Debug, Args)]
pub struct SessionIdArgs {
    /// Session ID.
    pub id: String,
}

/// Arguments for the A2A agent server.
#[derive(Debug, Args)]
pub struct A2aArgs {
    /// Port to bind the A2A server to (0 = OS-assigned).
    #[arg(long, default_value_t = 0)]
    pub port: u16,

    /// Mesh key for mDNS discovery isolation (defaults to a cwd-derived key).
    #[arg(long)]
    pub mesh_key: Option<String>,

    #[command(flatten)]
    pub ui: UiArgs,
}

/// TUI presentation flags shared by the default mode and the A2A server.
///
/// The ACP server renders no TUI, so it does not accept these flags.
#[derive(Debug, Args)]
pub struct UiArgs {
    /// TUI theme: catppuccin-mocha, catppuccin-latte, catppuccin-frappe, catppuccin-macchiato.
    #[arg(long)]
    pub theme: Option<String>,
}

/// Flags shared by every runtime mode.
#[derive(Debug, Args)]
pub struct AgentArgs {
    /// Model in provider/model format (e.g. 'openai/gpt-4o').
    #[arg(short = 'm', long, global = true)]
    pub model: Option<String>,

    /// API key override for the provider.
    #[arg(long, global = true)]
    pub api_key: Option<String>,

    /// Custom API endpoint URL.
    #[arg(long, global = true)]
    pub base_url: Option<String>,

    /// Sampling temperature (0.0 to 2.0).
    #[arg(long, global = true)]
    pub temperature: Option<f64>,

    /// Maximum context window size (input + output).
    #[arg(long, global = true)]
    pub max_context_tokens: Option<u64>,

    /// Maximum output tokens.
    #[arg(long, global = true)]
    pub max_output_tokens: Option<u64>,

    /// Maximum tool calling turns.
    #[arg(long, global = true)]
    pub max_turns: Option<u32>,

    /// Session ID to use (auto-creates if it does not exist).
    #[arg(long, global = true)]
    pub session: Option<String>,

    /// Timeout for tool execution (e.g. '30sec').
    #[arg(short = 't', long, global = true)]
    pub tool_timeout: Option<String>,

    /// Enable or disable the repetition guard (on/off).
    #[arg(long, global = true)]
    pub repetition_guard: Option<String>,

    /// Session store backend: sqlite, jsonl, or memory.
    #[arg(long, global = true)]
    pub store: Option<String>,

    /// Increase UX detail; repeat for more detail (-v, -vv, -vvv).
    #[arg(short = 'v', long, action = ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Suppress non-essential UX progress output.
    #[arg(short = 'q', long, global = true)]
    pub quiet: bool,

    /// Log level for file-based logging (error|warn|info|debug|trace).
    #[arg(long, global = true)]
    pub log_level: Option<String>,

    /// Agent persona name (loads .agents/<name>.md).
    #[arg(long, global = true)]
    pub agent: Option<String>,

    /// Agent instance identity for multi-agent messaging.
    #[arg(long, global = true)]
    pub name: Option<String>,

    /// Compaction strategy: sliding_summary, sliding_window, token_truncate.
    #[arg(long, global = true)]
    pub compaction_strategy: Option<String>,

    /// Number of recent messages to keep during compaction.
    #[arg(long, global = true)]
    pub keep_recent: Option<u32>,

    /// Token budget for the token_truncate strategy.
    #[arg(long, global = true)]
    pub token_budget: Option<u64>,

    /// Proactive compaction threshold percentage (0.0 to 1.0).
    #[arg(long, global = true)]
    pub proactive_threshold_pct: Option<f64>,
}

// endregion: --- Types

fn main() {
    // install_default returns Err(Arc<CryptoProvider>) only when a provider is
    // already installed (e.g. by an embedded runtime) — that is not fatal.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let Cli {
        command,
        prompt,
        ui,
        args,
    } = Cli::parse();

    init_logging(&args);

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("agent: failed to start runtime: {err}");
            std::process::exit(1);
        }
    };

    let result = runtime.block_on(async {
        match command {
            Some(Command::Acp) => dispatch::run_acp().await,
            Some(Command::A2a(a2a)) => dispatch::run_a2a(&args, &a2a).await,
            Some(Command::Config(cmd)) => dispatch::run_config_command(&cmd),
            Some(Command::Models(cmd)) => dispatch::run_models_command(&cmd),
            Some(Command::Provider(cmd)) => dispatch::run_provider_command(&cmd).await,
            Some(Command::Mcp(cmd)) => dispatch::run_mcp_command(&cmd).await,
            Some(Command::Session(cmd)) => dispatch::run_session_command(&args, &cmd).await,
            None => {
                // Only the default mode consumes stdin as a prompt; ACP owns
                // stdin as its protocol channel.
                let prompt = resolve_prompt(prompt);
                dispatch::run_agent(&args, prompt, &ui).await
            }
        }
    });

    if let Err(err) = result {
        eprintln!("agent: {err}");
        std::process::exit(1);
    }
}

// region:    --- Support

/// Resolve the single-turn prompt from the positional arg or piped stdin.
///
/// Returns `None` when no positional arg is given and stdin is a terminal —
/// the caller then starts the interactive TUI.
fn resolve_prompt(positional: Option<String>) -> Option<String> {
    if positional.is_some() {
        return positional;
    }
    if std::io::stdin().is_terminal() {
        return None;
    }

    use std::io::Read;
    let mut buffer = String::new();
    if std::io::stdin().read_to_string(&mut buffer).is_err() {
        return None;
    }
    let trimmed = buffer.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn init_logging(args: &AgentArgs) {
    let mut builder =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"));

    if let Some(level) = args.log_level.as_deref() {
        builder.filter_level(level.parse().unwrap_or(log::LevelFilter::Info));
    }

    // Log to a file, never to stderr. The TUI renders on stderr, so stderr log
    // output corrupts the display and reappears on the terminal once the TUI
    // leaves the alternate screen.
    builder.target(log_target());
    builder.format_timestamp_millis();
    builder.init();
}

/// Open the file log target at `$XDG_STATE_HOME/nu-agent/logs/agent.log`.
///
/// Falls back to a sink when the log directory or file cannot be opened, so
/// that logging never writes to stderr.
fn log_target() -> env_logger::Target {
    let sink = || env_logger::Target::Pipe(Box::new(std::io::sink()));

    let Ok(state_dir) = agnt_core::utils::xdg::state_dir() else {
        return sink();
    };
    let log_dir = state_dir.join("nu-agent").join("logs");
    if std::fs::create_dir_all(&log_dir).is_err() {
        return sink();
    }
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("agent.log"))
    else {
        return sink();
    };

    env_logger::Target::Pipe(Box::new(file))
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../test/version.rs"]
mod version_test;

#[cfg(test)]
#[path = "../test/cli.rs"]
mod cli_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../test/integration.rs"]
mod integration_test;

// endregion: --- Tests
