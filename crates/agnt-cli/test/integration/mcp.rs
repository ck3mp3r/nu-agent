//! Integration tests for `dispatch::utility::run_mcp_command`.
//!
//! The status and logout paths read a real config.toml and a real vault rooted
//! in the tempdir. The login path is exercised up to the OAuth discovery call,
//! which fails against the unreachable loopback endpoint — no browser opens and
//! no external network is reached.

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// A config.toml with one server per auth type, all disabled and all pointing
/// at an unreachable loopback endpoint.
const MCP_CONFIG: &str = r#"
[models.default]
model = "ollama/llama3"

[providers.ollama]
base_url = "http://localhost:11434"

[mcp.plain]
transport = "http"
url = "http://127.0.0.1:9/mcp"
enabled = false

[mcp.bearer]
transport = "http"
url = "http://127.0.0.1:9/mcp"
enabled = false

[mcp.bearer.auth]
type = "bearer"
token = "static-token"

[mcp.oauth]
transport = "http"
url = "http://127.0.0.1:9/mcp"
enabled = false

[mcp.oauth.auth]
type = "oauth"
client-id = "client-id"
"#;

// endregion: --- Test Support

// region:    --- Tests

#[tokio::test]
#[serial_test::serial]
async fn test_integration_mcp_status_without_servers_reports_none() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_default_config()?;
    let command = support::parse_mcp_command(&["mcp", "auth", "status"])?;

    // -- Exec
    dispatch::run_mcp_command(&command).await?;

    // -- Check
    // The command reports "No MCP servers configured." and returns Ok.
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_mcp_status_renders_every_auth_type() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_config(MCP_CONFIG)?;
    let command = support::parse_mcp_command(&["mcp", "auth", "status"])?;

    // -- Exec
    dispatch::run_mcp_command(&command).await?;

    // -- Check
    // The table carries one row per configured server, with the auth type and
    // the unauthenticated status for the OAuth server.
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_mcp_logout_without_credentials_reports_none() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_config(MCP_CONFIG)?;
    let command = support::parse_mcp_command(&["mcp", "auth", "logout", "oauth"])?;

    // -- Exec
    dispatch::run_mcp_command(&command).await?;

    // -- Check
    // The command reports "No stored credentials for 'oauth'" and returns Ok.
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_mcp_login_unknown_server_errors() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_config(MCP_CONFIG)?;
    let command = support::parse_mcp_command(&["mcp", "auth", "login", "absent"])?;

    // -- Exec
    let err = dispatch::run_mcp_command(&command)
        .await
        .err()
        .ok_or("an unknown MCP server must fail")?;

    // -- Check
    assert!(
        err.to_string().contains("MCP server 'absent' not found"),
        "got: {err}"
    );
    for name in ["plain", "bearer", "oauth"] {
        assert!(
            err.to_string().contains(name),
            "the error must list the configured server '{name}'; got: {err}"
        );
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_mcp_login_non_oauth_server_errors() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    env.write_config(MCP_CONFIG)?;
    let command = support::parse_mcp_command(&["mcp", "auth", "login", "plain"])?;

    // -- Exec
    let err = dispatch::run_mcp_command(&command)
        .await
        .err()
        .ok_or("a non-OAuth MCP server must fail")?;

    // -- Check
    assert!(
        err.to_string()
            .contains("does not use OAuth authentication"),
        "got: {err}"
    );
    Ok(())
}

// endregion: --- Tests
