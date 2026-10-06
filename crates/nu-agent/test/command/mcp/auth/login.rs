use nu_agent_core::tools::mcp::config::{McpAuthConfig, McpServerConfig, McpTransportType};

use super::login::validate_login_config;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn make_oauth_server(name: &str, url: Option<&str>) -> McpServerConfig {
    McpServerConfig {
        name: name.to_string(),
        transport: McpTransportType::Sse,
        enabled: true,
        url: url.map(|s| s.to_string()),
        auth: McpAuthConfig::OAuth {
            client_id: Some("my-client".to_string()),
            client_secret: None,
            scope: Some("read write".to_string()),
            redirect_uri: None,
        },
        headers: Default::default(),
        command: None,
        cwd: None,
        args: vec![],
        env: Default::default(),
    }
}

fn make_bearer_server(name: &str) -> McpServerConfig {
    McpServerConfig {
        name: name.to_string(),
        transport: McpTransportType::Sse,
        enabled: true,
        url: Some("https://example.com/mcp".to_string()),
        auth: McpAuthConfig::Bearer {
            token: "my-token".to_string(),
        },
        headers: Default::default(),
        command: None,
        cwd: None,
        args: vec![],
        env: Default::default(),
    }
}

fn make_none_auth_server(name: &str) -> McpServerConfig {
    McpServerConfig {
        name: name.to_string(),
        transport: McpTransportType::Sse,
        enabled: true,
        url: Some("https://example.com/mcp".to_string()),
        auth: McpAuthConfig::None,
        headers: Default::default(),
        command: None,
        cwd: None,
        args: vec![],
        env: Default::default(),
    }
}

#[test]
fn login_validates_oauth_server_config() {
    // validate_login_config is called after the server is found by name.
    // The "not found" case is handled by the caller (run_inner).
    // Here we verify the validation function accepts a valid OAuth server.
    let server = make_oauth_server("my-server", Some("https://example.com/mcp"));
    let result = validate_login_config(&server);
    assert!(result.is_ok());
}

#[test]
fn login_errors_when_server_does_not_use_oauth() {
    let bearer_server = make_bearer_server("bearer-server");
    let result = validate_login_config(&bearer_server);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("does not use OAuth"));
    assert!(err.contains("bearer-server"));

    let none_server = make_none_auth_server("none-server");
    let result = validate_login_config(&none_server);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("does not use OAuth"));
    assert!(err.contains("none-server"));
}

#[test]
fn login_errors_when_server_has_no_url() {
    let server = make_oauth_server("no-url-server", None);
    let result = validate_login_config(&server);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("no URL configured"));
    assert!(err.contains("no-url-server"));
}

#[test]
fn login_succeeds_with_valid_oauth_config() -> Result<()> {
    let server = make_oauth_server("valid-server", Some("https://example.com/mcp"));
    let result = validate_login_config(&server);

    let (url, auth) = result?;
    assert_eq!(url, "https://example.com/mcp");
    match auth {
        McpAuthConfig::OAuth {
            client_id, scope, ..
        } => {
            assert_eq!(client_id.as_deref(), Some("my-client"));
            assert_eq!(scope.as_deref(), Some("read write"));
        }
        _ => panic!("expected OAuth config"),
    }
    Ok(())
}

#[test]
fn login_succeeds_with_oauth_no_client_id() -> Result<()> {
    let server = McpServerConfig {
        name: "dynamic-server".to_string(),
        transport: McpTransportType::Sse,
        enabled: true,
        url: Some("https://example.com/mcp".to_string()),
        auth: McpAuthConfig::OAuth {
            client_id: None,
            client_secret: None,
            scope: None,
            redirect_uri: None,
        },
        headers: Default::default(),
        command: None,
        cwd: None,
        args: vec![],
        env: Default::default(),
    };
    let result = validate_login_config(&server);

    let (url, auth) = result?;
    assert_eq!(url, "https://example.com/mcp");
    match auth {
        McpAuthConfig::OAuth { client_id, .. } => {
            assert!(client_id.is_none());
        }
        _ => panic!("expected OAuth config"),
    }
    Ok(())
}
