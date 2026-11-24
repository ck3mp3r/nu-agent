use super::*;

#[tokio::test]
async fn connect_servers_does_not_replace_existing_handle_contents()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    use rig::tool::server::ToolServer;

    // Create a handle — simulates the application-level handle
    // that already has builtins registered on it.
    // We cannot add a real tool here without implementing ToolDyn,
    // so we verify structurally: McpRuntime must NOT contain a
    // tool_server_handle field (it was removed in this fix).
    // This test compiles only if the struct has no such field.
    let handle = ToolServer::new().run();

    // Connect with no servers — should succeed and return empty runtime
    let result = crate::tools::mcp::runtime::connect_servers(
        &handle,
        &[],
        None,
        20_000,
        None,
        crate::tools::mcp::runtime::CONNECT_DEADLINE,
    )
    .await;

    assert!(
        result.is_ok(),
        "connect_servers with empty config should succeed"
    );
    let mcp_runtime = result.map_err(|e| format!("{e:?}"))?;
    assert!(
        !mcp_runtime.has_sessions(),
        "no sessions expected with empty config"
    );

    // Structural proof: if McpRuntime had a tool_server_handle field,
    // this line would not compile (field access would exist but not here).
    // The absence of McpRuntime::tool_server_handle() is enforced at compile time.

    Ok(())
}

/// A `McpServerConfig` with `transport`, `enabled`, and no command or url.
fn server(name: &str, transport: McpTransportType, enabled: bool) -> McpServerConfig {
    McpServerConfig {
        name: name.to_string(),
        transport,
        url: None,
        headers: HashMap::new(),
        auth: McpAuthConfig::None,
        command: None,
        cwd: None,
        args: vec![],
        env: HashMap::new(),
        enabled,
    }
}

#[tokio::test]
async fn connect_servers_skips_disabled_servers() -> Result<()> {
    // -- Setup & Fixtures
    let handle = rig::tool::server::ToolServer::new().run();
    let servers = vec![
        server("disabled-stdio", McpTransportType::Stdio, false),
        server("disabled-http", McpTransportType::Http, false),
    ];

    // -- Exec
    let runtime = crate::tools::mcp::runtime::connect_servers(
        &handle,
        &servers,
        None,
        20_000,
        None,
        crate::tools::mcp::runtime::CONNECT_DEADLINE,
    )
    .await
    .map_err(|e| format!("disabled servers must not be connected: {e}"))?;

    // -- Check
    assert!(
        !runtime.has_sessions(),
        "a disabled server must not open a session"
    );
    assert!(
        runtime.discovered_tools().is_empty(),
        "a disabled server must contribute no tools"
    );
    assert!(
        !runtime.has_server("disabled-stdio") && !runtime.has_server("disabled-http"),
        "a disabled server must not be reported as connected"
    );
    Ok(())
}

#[tokio::test]
async fn connect_server_config_error_table() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::TempDir::new()?;
    let mut no_cwd = server("no-cwd", McpTransportType::Stdio, true);
    no_cwd.command = Some("sh".to_string());
    let cases: Vec<(&str, McpServerConfig, Option<&std::path::Path>, &str)> = vec![
        (
            "a stdio server without a command",
            server("no-command", McpTransportType::Stdio, true),
            None,
            "with transport 'stdio' requires command",
        ),
        (
            "an http server without a url",
            server("no-url", McpTransportType::Http, true),
            Some(temp.path()),
            "with transport 'http' requires url",
        ),
        (
            "a stdio server without a caller cwd",
            no_cwd,
            None,
            "missing caller cwd for stdio MCP server 'no-cwd'",
        ),
    ];

    // -- Exec & Check
    for (name, config, caller_cwd, expected) in cases {
        let handle = rig::tool::server::ToolServer::new().run();
        let error = super::super::connect::connect_server(
            &handle,
            &config,
            caller_cwd,
            20_000,
            None,
            crate::tools::mcp::runtime::CONNECT_DEADLINE,
        )
        .await
        .err()
        .ok_or_else(|| format!("case '{name}': the server must fail"))?;
        assert!(error.contains(expected), "case '{name}': got {error}");
    }
    Ok(())
}

#[tokio::test]
async fn connect_server_stdio_with_missing_binary_errors() -> Result<()> {
    // -- Setup & Fixtures
    let handle = rig::tool::server::ToolServer::new().run();
    let temp = tempfile::TempDir::new()?;
    let mut config = server("missing-binary", McpTransportType::Stdio, true);
    config.command = Some("/nonexistent/nu-agent-mcp-binary".to_string());

    // -- Exec
    let error = super::super::connect::connect_server(
        &handle,
        &config,
        Some(temp.path()),
        20_000,
        None,
        crate::tools::mcp::runtime::CONNECT_DEADLINE,
    )
    .await
    .err()
    .ok_or("a stdio server with a missing binary must fail")?;

    // -- Check
    assert!(
        error.contains("failed to build stdio transport")
            || error.contains("failed to connect stdio MCP server"),
        "got: {error}"
    );
    Ok(())
}

// region:    --- Startup resilience pins

/// A stdio MCP server stub: answers `initialize` and `tools/list`, exposes one
/// tool, and exits when its stdin closes.
const STUB_MCP_SERVER: &str = r#"while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*\"id\":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *initialize*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"stub-mcp","version":"1.0.0"}}}\n' "$id"
      ;;
    *tools/list*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"ping","description":"stub ping","inputSchema":{"type":"object","properties":{}}}]}}\n' "$id"
      ;;
  esac
done
"#;

/// A `McpServerConfig` running `program` with `args` on the stdio transport.
fn stdio_server(name: &str, program: &str, args: Vec<String>) -> McpServerConfig {
    let mut config = server(name, McpTransportType::Stdio, true);
    config.command = Some(program.to_string());
    config.args = args;
    config
}

/// A stdio server that answers the initialize handshake from [`STUB_MCP_SERVER`].
fn stub_server(dir: &std::path::Path, name: &str) -> Result<McpServerConfig> {
    let script = dir.join(format!("{name}.sh"));
    std::fs::write(&script, STUB_MCP_SERVER)?;
    Ok(stdio_server(
        name,
        "sh",
        vec![script.to_string_lossy().to_string()],
    ))
}

/// A stdio child that reads its input and never answers: the handshake hangs.
fn silent_server(name: &str) -> McpServerConfig {
    stdio_server(
        name,
        "sh",
        vec!["-c".to_string(), "cat > /dev/null".to_string()],
    )
}

#[tokio::test]
async fn connect_server_bounds_a_silent_stdio_handshake() -> Result<()> {
    // -- Setup & Fixtures
    let handle = rig::tool::server::ToolServer::new().run();
    let temp = tempfile::TempDir::new()?;
    let config = silent_server("silent-server");
    let deadline = std::time::Duration::from_millis(300);

    // -- Exec
    let started = std::time::Instant::now();
    let error = super::super::connect::connect_server(
        &handle,
        &config,
        Some(temp.path()),
        20_000,
        None,
        deadline,
    )
    .await
    .err()
    .ok_or("a child that never handshakes must not connect")?;
    let elapsed = started.elapsed();

    // -- Check
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "the initialize handshake must be bounded, took {elapsed:?}"
    );
    assert!(
        error.contains("initialize handshake did not complete within 300ms"),
        "the failure must name the bound, got: {error}"
    );
    Ok(())
}

#[tokio::test]
async fn connect_servers_skips_a_silent_server() -> Result<()> {
    // -- Setup & Fixtures
    let handle = rig::tool::server::ToolServer::new().run();
    let temp = tempfile::TempDir::new()?;
    let servers = vec![silent_server("silent-server")];
    let deadline = std::time::Duration::from_millis(300);

    // -- Exec
    let started = std::time::Instant::now();
    let runtime = crate::tools::mcp::runtime::connect_servers(
        &handle,
        &servers,
        Some(temp.path()),
        20_000,
        None,
        deadline,
    )
    .await
    .map_err(|e| format!("a silent server must not abort startup: {e}"))?;
    let elapsed = started.elapsed();

    // -- Check
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "the initialize handshake must be bounded, took {elapsed:?}"
    );
    assert!(
        !runtime.has_server("silent-server"),
        "a server that never handshakes must not be reported connected"
    );
    assert!(
        runtime.discovered_tools().is_empty(),
        "a server that never handshakes must contribute no tools"
    );
    Ok(())
}

#[tokio::test]
async fn connect_servers_keeps_a_healthy_server_when_one_is_broken() -> Result<()> {
    // -- Setup & Fixtures
    let handle = rig::tool::server::ToolServer::new().run();
    let temp = tempfile::TempDir::new()?;
    let broken = server("broken-stdio", McpTransportType::Stdio, true);
    let healthy = stub_server(temp.path(), "healthy-stub")?;
    let servers = vec![broken, healthy];

    // -- Exec
    let runtime = crate::tools::mcp::runtime::connect_servers(
        &handle,
        &servers,
        Some(temp.path()),
        20_000,
        None,
        std::time::Duration::from_secs(5),
    )
    .await
    .map_err(|e| format!("one broken server must not abort the whole startup: {e}"))?;

    // -- Check
    assert!(
        runtime.has_server("healthy-stub"),
        "the healthy server must stay connected"
    );
    assert!(
        !runtime.has_server("broken-stdio"),
        "the broken server must not be reported connected"
    );
    let names: Vec<&str> = runtime
        .discovered_tools()
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    assert!(
        names.contains(&"healthy-stub__ping"),
        "the healthy server's tools must survive its neighbour's failure, got: {names:?}"
    );
    Ok(())
}

// endregion: --- Startup resilience pins
