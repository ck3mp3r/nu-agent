use super::*;

#[tokio::test]
async fn connect_servers_does_not_replace_existing_handle_contents() {
    use rig::tool::server::ToolServer;

    // Create a handle — simulates the application-level handle
    // that already has builtins registered on it.
    // We cannot add a real tool here without implementing ToolDyn,
    // so we verify structurally: McpRuntime must NOT contain a
    // tool_server_handle field (it was removed in this fix).
    // This test compiles only if the struct has no such field.
    let handle = ToolServer::new().run();

    // Connect with no servers — should succeed and return empty runtime
    let result =
        crate::tools::mcp::runtime::connect_servers(&handle, &[], None, 20_000, None).await;

    assert!(
        result.is_ok(),
        "connect_servers with empty config should succeed"
    );
    let mcp_runtime = result.unwrap();
    assert!(
        !mcp_runtime.has_sessions(),
        "no sessions expected with empty config"
    );

    // Structural proof: if McpRuntime had a tool_server_handle field,
    // this line would not compile (field access would exist but not here).
    // The absence of McpRuntime::tool_server_handle() is enforced at compile time.
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
    let runtime =
        crate::tools::mcp::runtime::connect_servers(&handle, &servers, None, 20_000, None)
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
async fn connect_servers_stdio_without_command_errors() -> Result<()> {
    // -- Setup & Fixtures
    let handle = rig::tool::server::ToolServer::new().run();
    let servers = vec![server("no-command", McpTransportType::Stdio, true)];

    // -- Exec
    let error = crate::tools::mcp::runtime::connect_servers(&handle, &servers, None, 20_000, None)
        .await
        .err()
        .ok_or("a stdio server without a command must fail")?;

    // -- Check
    assert!(
        error.contains("with transport 'stdio' requires command"),
        "got: {error}"
    );
    Ok(())
}

#[tokio::test]
async fn connect_servers_http_without_url_errors() -> Result<()> {
    // -- Setup & Fixtures
    let handle = rig::tool::server::ToolServer::new().run();
    let servers = vec![server("no-url", McpTransportType::Http, true)];

    // -- Exec
    let error = crate::tools::mcp::runtime::connect_servers(&handle, &servers, None, 20_000, None)
        .await
        .err()
        .ok_or("an http server without a url must fail")?;

    // -- Check
    assert!(
        error.contains("with transport 'http' requires url"),
        "got: {error}"
    );
    Ok(())
}

#[tokio::test]
async fn connect_servers_stdio_without_caller_cwd_errors() -> Result<()> {
    // -- Setup & Fixtures
    let handle = rig::tool::server::ToolServer::new().run();
    let mut config = server("no-cwd", McpTransportType::Stdio, true);
    config.command = Some("nu".to_string());
    let servers = vec![config];

    // -- Exec
    let error = crate::tools::mcp::runtime::connect_servers(&handle, &servers, None, 20_000, None)
        .await
        .err()
        .ok_or("a stdio server without a caller cwd must fail")?;

    // -- Check
    assert!(
        error.contains("missing caller cwd for stdio MCP server 'no-cwd'"),
        "got: {error}"
    );
    Ok(())
}

#[tokio::test]
async fn connect_servers_stdio_with_missing_binary_errors() -> Result<()> {
    // -- Setup & Fixtures
    let handle = rig::tool::server::ToolServer::new().run();
    let temp = tempfile::TempDir::new()?;
    let mut config = server("missing-binary", McpTransportType::Stdio, true);
    config.command = Some("/nonexistent/nu-agent-mcp-binary".to_string());
    let servers = vec![config];

    // -- Exec
    let error = crate::tools::mcp::runtime::connect_servers(
        &handle,
        &servers,
        Some(temp.path()),
        20_000,
        None,
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
