//! Code-level integration tests for `build_acp_runtime()`.
//!
//! Every test calls `build_acp_runtime()` directly from Rust and inspects the
//! returned `AgentConversationRuntime`. No subprocess, no shell, no pipes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agnt_core::session::factory::create_store_in;
use agnt_core::session::{SessionStore, SessionStoreBackend, StoreType};
use agnt_core::tools::handler::is_builtin_tool_name;
use agnt_core::tools::mcp::client::McpToolDefinition;
use agnt_core::tools::mcp::config::{McpAuthConfig, McpServerConfig, McpTransportType};

use agent_client_protocol::schema::v1::FileSystemCapabilities;
use agent_client_protocol::{Agent, Channel, Client, ConnectionTo};

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

// ---------------------------------------------------------------------------
// client filesystem capabilities
// ---------------------------------------------------------------------------

/// A live in-memory `ConnectionTo<Client>` for a `ClientFsState`.
///
/// The connection runs over a duplex channel: no transport, no network, no
/// subprocess. The client task owns the peer endpoint, so the channel stays
/// open for the test's lifetime.
async fn client_connection() -> Result<ConnectionTo<Client>> {
    let (agent_channel, client_channel) = Channel::duplex();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let _ = Agent
            .builder()
            .connect_with(agent_channel, async move |cx| {
                let _ = tx.send(cx);
                std::future::pending::<()>().await;
                Ok(())
            })
            .await;
    });
    let connection = rx.await?;
    tokio::spawn(async move {
        let _ = Client
            .builder()
            .name("agnt-acp-runtime-test-client")
            .connect_to(client_channel)
            .await;
    });
    Ok(connection)
}

fn client_fs_state(
    connection: ConnectionTo<Client>,
    capabilities: FileSystemCapabilities,
) -> ClientFsState {
    ClientFsState {
        connection,
        capabilities,
    }
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_registers_client_fs_tools() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    // Enable the info level so the capability-summary log arguments are
    // evaluated; without a logger the `log::info!` arguments are skipped.
    log::set_max_level(log::LevelFilter::Info);
    let temp = tempfile::tempdir()?;
    let config_path = write_config(temp.path(), BASE_CONFIG)?;
    let store = memory_store(temp.path()).await?;
    let connection = client_connection().await?;
    let capabilities = FileSystemCapabilities::new()
        .read_text_file(true)
        .write_text_file(true);
    let mut params = make_params(temp.path().to_path_buf(), config_path, Vec::new(), store);
    params.client_fs = Some(client_fs_state(connection, capabilities));

    // -- Exec
    let runtime = build_acp_runtime(params)
        .await
        .map_err(|e| format!("should build runtime: {e:?}"))?;
    let names = visible_tool_names(&runtime);

    // -- Check
    for expected in ["read", "edit"] {
        assert!(
            names.iter().any(|name| name == expected),
            "client-fs '{expected}' tool must be registered; got: {names:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_client_fs_read_only_keeps_local_tools() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let config_path = write_config(temp.path(), BASE_CONFIG)?;
    let store = memory_store(temp.path()).await?;
    let connection = client_connection().await?;
    // Only the read capability: the client cannot write, so the local
    // filesystem tools must stay in place.
    let capabilities = FileSystemCapabilities::new().read_text_file(true);
    let mut params = make_params(temp.path().to_path_buf(), config_path, Vec::new(), store);
    params.client_fs = Some(client_fs_state(connection, capabilities));

    // -- Exec
    let runtime = build_acp_runtime(params)
        .await
        .map_err(|e| format!("should build runtime: {e:?}"))?;
    let names = visible_tool_names(&runtime);

    // -- Check
    for expected in ["read", "edit"] {
        assert!(
            names.iter().any(|name| name == expected),
            "local '{expected}' tool must stay registered; got: {names:?}"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// AGENTS.md load warnings
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_runtime_build_acp_runtime_unreadable_agents_md_is_non_fatal() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    // Enable the info level so the warning log arguments are evaluated.
    log::set_max_level(log::LevelFilter::Info);
    let temp = tempfile::tempdir()?;
    let cwd = temp.path().join("session-cwd");
    std::fs::create_dir_all(&cwd)?;
    let agents_md = cwd.join("AGENTS.md");
    std::fs::write(&agents_md, "UNREADABLE_MARKER")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&agents_md)?.permissions();
        perms.set_mode(0o000);
        std::fs::set_permissions(&agents_md, perms)?;
    }
    let config_path = write_config(temp.path(), BASE_CONFIG)?;
    let store = memory_store(temp.path()).await?;

    // -- Exec
    let runtime = build_acp_runtime(make_params(cwd.clone(), config_path, Vec::new(), store))
        .await
        .map_err(|e| {
            format!("an unreadable AGENTS.md must not abort runtime construction: {e:?}")
        })?;

    // -- Check
    let chain = runtime.persona.cached_agents_chain().unwrap_or("");
    assert!(
        !chain.contains("UNREADABLE_MARKER"),
        "an unreadable AGENTS.md must not contribute to the chain, got: {chain}"
    );

    // Cleanup: restore permissions so the temp dir can be removed.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&agents_md)?.permissions();
        perms.set_mode(0o644);
        std::fs::set_permissions(&agents_md, perms)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// config load error branches
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_runtime_build_acp_runtime_malformed_config_returns_config_error() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let config_path = write_config(temp.path(), "this is not = valid toml [[[")?;
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
    let err = result.err().ok_or("a malformed config.toml must fail")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "a malformed config.toml must produce AcpError::Config, got: {err:?}"
    );
    assert!(
        err.to_string().contains("failed to load config"),
        "error must mention the config load failure, got: {err}"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_missing_default_model_returns_config_error() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let config = "[providers.ollama]\nbase_url = \"http://localhost:11434\"\n";
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
    let err = result
        .err()
        .ok_or("a config without models.default must fail")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "a config without models.default must produce AcpError::Config, got: {err:?}"
    );
    assert!(
        err.to_string().contains("no model configured"),
        "error must mention the missing model, got: {err}"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_unresolvable_model_returns_config_error() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    // An empty model name cannot be resolved into a runtime config.
    let config = "[models.default]\nmodel = \"ollama/\"\n";
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
    let err = result.err().ok_or("an unresolvable model must fail")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "an unresolvable model must produce AcpError::Config, got: {err:?}"
    );
    assert!(
        err.to_string().contains("failed to resolve model"),
        "error must mention the resolve failure, got: {err}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// MCP tool definition mapping
// ---------------------------------------------------------------------------

#[test]
fn test_runtime_mcp_tool_definition_uses_tool_metadata() -> Result<()> {
    // -- Setup & Fixtures
    let tool = McpToolDefinition {
        server: "srv".to_string(),
        name: "mcp__srv__search".to_string(),
        raw_name: "search".to_string(),
        description: Some("Search things".to_string()),
        parameters: Some(serde_json::json!({"type": "object"})),
    };

    // -- Exec
    let def = mcp_tool_definition(&tool).map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(def.name.to_string(), "mcp__srv__search");
    assert_eq!(def.description, "Search things");
    assert_eq!(def.parameters, serde_json::json!({"type": "object"}));
    Ok(())
}

#[test]
fn test_runtime_mcp_tool_definition_falls_back_to_defaults() -> Result<()> {
    // -- Setup & Fixtures
    let tool = McpToolDefinition {
        server: "srv".to_string(),
        name: "mcp__srv__ping".to_string(),
        raw_name: "ping".to_string(),
        description: None,
        parameters: None,
    };

    // -- Exec
    let def = mcp_tool_definition(&tool).map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(def.description, "MCP tool from server 'srv'");
    assert_eq!(def.parameters["type"], "object");
    assert_eq!(def.parameters["required"][0], "args");
    Ok(())
}

#[test]
fn test_runtime_mcp_tool_definition_rejects_empty_tool_name() {
    // -- Setup & Fixtures
    let tool = McpToolDefinition {
        server: "srv".to_string(),
        name: String::new(),
        raw_name: String::new(),
        description: None,
        parameters: None,
    };

    // -- Exec
    let result = mcp_tool_definition(&tool);

    // -- Check
    assert!(result.is_err(), "an empty MCP tool name must be rejected");
}

#[tokio::test]
async fn test_runtime_create_acp_store_creates_sqlite_store() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    // SAFETY: nextest gives each test its own process, so no other thread
    // reads the environment concurrently.
    unsafe { std::env::set_var("XDG_CACHE_HOME", temp.path()) };

    // -- Exec
    let store = create_acp_store()
        .await
        .map_err(|e| format!("should create the ACP store: {e:?}"))?;

    // -- Check
    let db = temp.path().join("nu-agent").join("sessions.db");
    assert!(
        db.exists(),
        "the ACP store must be created under the XDG cache dir at {}",
        db.display()
    );
    assert!(
        store.list().await.map_err(|e| format!("{e:?}"))?.is_empty(),
        "a fresh store must hold no sessions"
    );
    Ok(())
}
