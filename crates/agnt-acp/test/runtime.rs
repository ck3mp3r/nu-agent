//! Code-level integration tests for `build_acp_runtime()`.
//!
//! Every test calls `build_acp_runtime()` directly from Rust and inspects the
//! returned `AgentConversationRuntime`. No subprocess, no shell, no pipes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agnt_core::session::factory::create_store_in;
use agnt_core::session::{SessionStoreBackend, StoreType};
use agnt_core::tools::handler::is_builtin_tool_name;
use agnt_core::tools::mcp::config::{McpAuthConfig, McpServerConfig, McpTransportType};

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Base config.toml: one `default` role on the `ollama` provider.
///
/// `ollama` builds its client from `base_url` alone — no credential, no
/// network call — so runtime construction stays offline.
const BASE_CONFIG: &str = r#"
[models.default]
model = "ollama/llama3"

[providers.ollama]
base_url = "http://localhost:11434"
"#;

fn write_config(dir: &Path, contents: &str) -> Result<PathBuf> {
    let path = dir.join("config.toml");
    std::fs::write(&path, contents)?;
    Ok(path)
}

/// Force the env-only vault backend so config load never probes the OS keychain.
///
/// nextest runs each test in its own process, so mutating the process
/// environment here cannot race with another test.
fn force_env_only_vault() {
    // SAFETY: nextest gives each test its own process, so no other thread
    // reads the environment concurrently.
    unsafe { std::env::set_var("AGENT_VAULT_BACKEND", "env") };
}

async fn memory_store(dir: &Path) -> Result<Arc<SessionStoreBackend>> {
    let store = create_store_in(StoreType::Memory, dir)
        .await
        .map_err(|e| format!("should create in-memory store: {e:?}"))?;
    Ok(Arc::new(store))
}

fn make_params(
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
    }
}

/// A disabled HTTP MCP server. Disabled servers are never connected, so the
/// runtime can be built without a reachable MCP endpoint.
fn disabled_http_server(name: &str) -> McpServerConfig {
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

fn visible_tool_names(runtime: &AgentConversationRuntime) -> Vec<String> {
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

fn lifecycle_names(runtime: &AgentConversationRuntime) -> Vec<String> {
    runtime
        .mcp_state
        .mcp_lifecycle_projection()
        .iter()
        .map(|server| server.name.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_runtime_build_acp_runtime_valid_config_returns_ok() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let cwd = temp.path().join("session-cwd");
    std::fs::create_dir_all(&cwd)?;
    let config_path = write_config(temp.path(), BASE_CONFIG)?;
    let store = memory_store(temp.path()).await?;

    // -- Exec
    let runtime = build_acp_runtime(make_params(cwd.clone(), config_path, Vec::new(), store))
        .await
        .map_err(|e| format!("should build runtime: {e:?}"))?;

    // -- Check
    assert_eq!(runtime.cwd, cwd, "runtime cwd must come from params");
    assert!(
        runtime.final_session_id.is_none(),
        "a fresh session has no persisted session id"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_installs_interactive_pending() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let config_path = write_config(temp.path(), BASE_CONFIG)?;
    let store = memory_store(temp.path()).await?;

    // -- Exec
    let runtime = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        config_path,
        Vec::new(),
        store,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;
    let pending = runtime.interactive_pending_arc();

    // -- Check
    assert!(
        pending.is_some(),
        "the ACP runtime must install the interactive pending map so execute_turn takes the interactive branch"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_missing_config_returns_config_error() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let missing = temp.path().join("does-not-exist.toml");
    let store = memory_store(temp.path()).await?;

    // -- Exec
    let result = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        missing,
        Vec::new(),
        store,
    ))
    .await;

    // -- Check
    let err = result.err().ok_or("missing config must fail")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "missing config must produce AcpError::Config, got: {err:?}"
    );
    assert!(
        err.to_string().contains("config"),
        "error message must mention config, got: {err}"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_invalid_config_returns_config_error() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    // The model id is synthetic so no models cache entry can clamp
    // max_output_tokens down and mask the invalid combination.
    let temp = tempfile::tempdir()?;
    let config = r#"
[models.default]
model = "ollama/acp-validation-probe"
max_output_tokens = 8192
max_context_tokens = 4096

[providers.ollama]
base_url = "http://localhost:11434"
"#;
    let config_path = write_config(temp.path(), config)?;
    let store = memory_store(temp.path()).await?;

    // -- Exec
    let result = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        config_path,
        Vec::new(),
        store,
    ))
    .await;

    // -- Check
    let err = result.err().ok_or("invalid config must fail")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "invalid config must produce AcpError::Config, got: {err:?}"
    );
    assert!(
        err.to_string().contains("config validation failed"),
        "error message must mention config validation, got: {err}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Tool registration
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_runtime_build_acp_runtime_registers_builtin_tools() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let config_path = write_config(temp.path(), BASE_CONFIG)?;
    let store = memory_store(temp.path()).await?;

    // -- Exec
    let runtime = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        config_path,
        Vec::new(),
        store,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;
    let names = visible_tool_names(&runtime);

    // -- Check
    for expected in ["read", "edit", "grep", "glob"] {
        assert!(
            names.iter().any(|name| name == expected),
            "built-in tool '{expected}' must be registered; got: {names:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_has_no_closure_tools() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let config_path = write_config(temp.path(), BASE_CONFIG)?;
    let store = memory_store(temp.path()).await?;

    // -- Exec
    let runtime = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        config_path,
        Vec::new(),
        store,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;
    let closure_count = runtime.tools.closure_registry().names().count();
    let names = visible_tool_names(&runtime);

    // -- Check
    assert_eq!(
        closure_count, 0,
        "ACP mode has no Nushell engine, so the closure registry must be empty"
    );
    let non_builtin: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|name| !is_builtin_tool_name(name))
        .collect();
    assert!(
        non_builtin.is_empty(),
        "every visible tool must be a built-in; found non-builtins: {non_builtin:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// MCP merge
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_runtime_build_acp_runtime_merges_distinct_mcp_servers() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let config = format!(
        "{BASE_CONFIG}\n[mcp.config_only]\ntransport = \"http\"\nurl = \"http://127.0.0.1:9/mcp\"\nenabled = false\n"
    );
    let config_path = write_config(temp.path(), &config)?;
    let store = memory_store(temp.path()).await?;
    let client_servers = vec![disabled_http_server("client_only")];

    // -- Exec
    let runtime = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        config_path,
        client_servers,
        store,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;
    let names = lifecycle_names(&runtime);

    // -- Check
    assert_eq!(
        names,
        vec!["client_only".to_string(), "config_only".to_string()],
        "servers unique to either source must all be present"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_client_mcp_server_wins_on_collision() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    // The config.toml server is enabled; the client server of the same name is
    // disabled. Only a client-wins merge leaves the merged entry disabled — and
    // therefore leaves the runtime constructible without a reachable endpoint.
    let temp = tempfile::tempdir()?;
    let config = format!(
        "{BASE_CONFIG}\n[mcp.shared]\ntransport = \"http\"\nurl = \"http://127.0.0.1:9/mcp\"\nenabled = true\n"
    );
    let config_path = write_config(temp.path(), &config)?;
    let store = memory_store(temp.path()).await?;
    let client_servers = vec![disabled_http_server("shared")];

    // -- Exec
    let runtime = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        config_path,
        client_servers,
        store,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;
    let projection = runtime.mcp_state.mcp_lifecycle_projection();

    // -- Check
    assert_eq!(
        projection.len(),
        1,
        "a name collision must collapse to one merged server, got: {projection:?}"
    );
    let shared = projection
        .first()
        .ok_or("merged projection must hold the shared server")?;
    assert_eq!(shared.name, "shared");
    assert!(
        !shared.enabled,
        "the client server must replace the config.toml server of the same name"
    );
    Ok(())
}
