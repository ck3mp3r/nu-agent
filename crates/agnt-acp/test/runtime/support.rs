//! Shared fixtures for the `build_acp_runtime()` integration tests.

use std::path::{Path, PathBuf};

use agnt_core::session::{RigStoreConfig, StoreType};
use agnt_core::tools::mcp::config::{McpAuthConfig, McpServerConfig, McpTransportType};

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Base config.toml: one `default` role on the `ollama` provider.
///
/// `ollama` builds its client from `base_url` alone — no credential, no
/// network call — so runtime construction stays offline.
pub(super) const BASE_CONFIG: &str = r#"
[models.default]
model = "ollama/llama3"

[providers.ollama]
base_url = "http://localhost:11434"
"#;

pub(super) fn write_config(dir: &Path, contents: &str) -> Result<PathBuf> {
    let path = dir.join("config.toml");
    std::fs::write(&path, contents)?;
    Ok(path)
}

/// Force the env-only vault backend so config load never probes the OS keychain.
///
/// nextest runs each test in its own process, so mutating the process
/// environment here cannot race with another test.
pub(super) fn force_env_only_vault() {
    // SAFETY: nextest gives each test its own process, so no other thread
    // reads the environment concurrently.
    unsafe { std::env::set_var("AGENT_VAULT_BACKEND", "env") };
}

/// The session store backend the runtime tests build against.
pub(super) fn memory_store_type() -> StoreType {
    StoreType::Memory
}

pub(super) fn make_params(
    cwd: PathBuf,
    config_path: PathBuf,
    mcp_servers: Vec<McpServerConfig>,
    store_type: StoreType,
) -> AcpRuntimeParams {
    AcpRuntimeParams {
        cwd,
        mcp_servers,
        paths: test_paths(&config_path),
        runtime_handle: tokio::runtime::Handle::current(),
        rig: RigStoreConfig::new(store_type),
        session_id: None,
        client_fs: None,
        agent_name: None,
        hook_context: agnt_core::hooks::HookAgentContext::default(),
    }
}

/// The application paths of a runtime test rooted at `config_path`'s dir.
pub(super) fn test_paths(config_path: &Path) -> agnt_core::config::Paths {
    let dir = config_path.parent().unwrap_or_else(|| Path::new("."));
    let logs = dir.join("logs");
    agnt_core::config::Paths {
        config_path: config_path.to_path_buf(),
        dotenv_path: dir.join(".env"),
        agents_dir: dir.join("agents"),
        copilot_token_dir: dir.join("github_copilot"),
        store_path: dir.join("rig-sessions.db"),
        log_dir: logs.clone(),
        log_path: logs.join("agent.log"),
        audit_log_path: dir.join("tool_audit.log"),
        secrets_path: dir.join("secrets.json"),
        models_cache_path: dir.join("models.json"),
        theme_path: dir.join("theme.json"),
        grammar_cache_dir: dir.join("tree-sitter"),
    }
}

/// A disabled HTTP MCP server. Disabled servers are never connected, so the
/// runtime can be built without a reachable MCP endpoint.
pub(super) fn disabled_http_server(name: &str) -> McpServerConfig {
    McpServerConfig {
        name: name.to_string(),
        transport: McpTransportType::Http,
        enabled: false,
        url: Some("http://127.0.0.1:9/mcp".to_string()),
        auth: McpAuthConfig::None,
        headers: Default::default(),
        command: None,
        cwd: None,
        args: Vec::new(),
        env: Default::default(),
    }
}

pub(super) fn visible_tool_names(runtime: &AgentConversationRuntime) -> Vec<String> {
    runtime
        .tools
        .active_definitions(
            runtime.mcp_state.mcp_registry(),
            runtime.permission_state.permissions(),
        )
        .into_iter()
        .map(|def| def.name.to_string())
        .collect()
}

pub(super) fn lifecycle_names(runtime: &AgentConversationRuntime) -> Vec<String> {
    runtime
        .mcp_state
        .mcp_lifecycle_projection()
        .iter()
        .map(|server| server.name.clone())
        .collect()
}

/// Write a persona file at `{cwd}/.agents/{name}.md`.
pub(super) fn write_persona(cwd: &Path, name: &str, contents: &str) -> Result<()> {
    let dir = cwd.join(".agents");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(format!("{name}.md")), contents)?;
    Ok(())
}

/// Point the XDG config lookup at `dir` so no host persona file can shadow the
/// test's `{cwd}/.agents` persona.
pub(super) fn isolate_xdg_config(dir: &Path) {
    // SAFETY: nextest gives each test its own process, so no other thread
    // reads the environment concurrently.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", dir) };
}

/// Captures log records so a test can assert on the permissions startup summary.
pub(super) struct CaptureLogger;

pub(super) static CAPTURED_LOGS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

impl log::Log for CaptureLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if let Ok(mut logs) = CAPTURED_LOGS.lock() {
            logs.push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}
