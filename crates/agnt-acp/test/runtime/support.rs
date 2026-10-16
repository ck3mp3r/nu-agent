//! Shared fixtures for the `build_acp_runtime()` integration tests.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agnt_core::session::factory::create_store_in;
use agnt_core::session::{SessionStoreBackend, StoreType};
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

pub(super) async fn memory_store(dir: &Path) -> Result<Arc<SessionStoreBackend>> {
    let store = create_store_in(StoreType::Memory, dir)
        .await
        .map_err(|e| format!("should create in-memory store: {e:?}"))?;
    Ok(Arc::new(store))
}

pub(super) fn make_params(
    cwd: PathBuf,
    config_path: PathBuf,
    mcp_servers: Vec<McpServerConfig>,
    store: Arc<SessionStoreBackend>,
) -> AcpRuntimeParams {
    AcpRuntimeParams {
        cwd,
        mcp_servers,
        config_path,
        runtime_handle: tokio::runtime::Handle::current(),
        store,
        session_id: None,
        client_fs: None,
        agent_name: None,
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
