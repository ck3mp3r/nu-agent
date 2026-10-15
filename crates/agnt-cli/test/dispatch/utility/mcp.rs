use super::*;

use agnt_core::tools::mcp::config::McpTransportType;
use oauth2::{AccessToken, basic::BasicTokenType};
use rmcp::transport::auth::{OAuthTokenResponse, VendorExtraTokenFields};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

fn make_creds_with_token(
    expires_in_secs: Option<u64>,
    received_at: Option<u64>,
) -> StoredCredentials {
    let mut resp = OAuthTokenResponse::new(
        AccessToken::new("test-token".to_string()),
        BasicTokenType::Bearer,
        VendorExtraTokenFields::default(),
    );
    if let Some(expires_in) = expires_in_secs {
        resp.set_expires_in(Some(std::time::Duration::from_secs(expires_in)).as_ref());
    }
    StoredCredentials::new("test-client".to_string(), Some(resp), vec![], received_at)
}

fn oauth_auth() -> McpAuthConfig {
    McpAuthConfig::OAuth {
        client_id: Some("cid".to_string()),
        client_secret: None,
        scope: None,
        redirect_uri: None,
    }
}

fn mcp_server(name: &str, url: Option<&str>, auth: McpAuthConfig) -> McpServerConfig {
    McpServerConfig {
        name: name.to_string(),
        transport: McpTransportType::Http,
        enabled: true,
        url: url.map(str::to_string),
        auth,
        headers: std::collections::HashMap::new(),
        command: None,
        cwd: None,
        args: vec![],
        env: std::collections::HashMap::new(),
    }
}

// endregion: --- Test Support

#[test]
fn test_utility_mcp_determine_status_none_auth() {
    // -- Exec
    let status = determine_status(&McpAuthConfig::None, None, 0);

    // -- Check
    assert_eq!(status, "no auth required");
}

#[test]
fn test_utility_mcp_determine_status_bearer_auth() {
    // -- Setup & Fixtures
    let auth = McpAuthConfig::Bearer {
        token: "my-token".to_string(),
    };

    // -- Exec
    let status = determine_status(&auth, None, 0);

    // -- Check
    assert_eq!(status, "static token (from config)");
}

#[test]
fn test_utility_mcp_determine_status_oauth_valid_token() {
    // -- Setup & Fixtures
    let creds = make_creds_with_token(Some(60), Some(100));

    // -- Exec
    let status = determine_status(&oauth_auth(), Some(&creds), 150);

    // -- Check
    assert_eq!(status, "authenticated (token valid)");
}

#[test]
fn test_utility_mcp_determine_status_oauth_expired_token() {
    // -- Setup & Fixtures
    let creds = make_creds_with_token(Some(60), Some(100));

    // -- Exec
    let status = determine_status(&oauth_auth(), Some(&creds), 200);

    // -- Check
    assert_eq!(status, "authenticated (token expired — will refresh)");
}

#[test]
fn test_utility_mcp_determine_status_oauth_without_credentials() {
    // -- Exec
    let status = determine_status(&oauth_auth(), None, 0);

    // -- Check
    assert_eq!(
        status,
        "not authenticated (run: agent mcp auth login <name>)"
    );
}

#[test]
fn test_utility_mcp_determine_status_oauth_without_token_response() {
    // -- Setup & Fixtures
    let creds = StoredCredentials::new("test-client".to_string(), None, vec![], None);

    // -- Exec
    let status = determine_status(&oauth_auth(), Some(&creds), 0);

    // -- Check
    assert_eq!(
        status,
        "not authenticated (run: agent mcp auth login <name>)"
    );
}

#[test]
fn test_utility_mcp_determine_status_oauth_without_expiry_is_valid() {
    // -- Setup & Fixtures
    let creds = make_creds_with_token(None, None);

    // -- Exec
    let status = determine_status(&oauth_auth(), Some(&creds), 999_999);

    // -- Check
    assert_eq!(status, "authenticated (token valid)");
}

#[test]
fn test_utility_mcp_validate_login_config_requires_oauth() -> Result<()> {
    // -- Setup & Fixtures
    let server = mcp_server("srv", Some("https://x.test/mcp"), McpAuthConfig::None);

    // -- Exec
    let err = validate_login_config(&server)
        .err()
        .ok_or("should reject non-OAuth server")?;

    // -- Check
    assert!(err.to_string().contains("does not use OAuth"), "err={err}");
    Ok(())
}

#[test]
fn test_utility_mcp_validate_login_config_requires_url() -> Result<()> {
    // -- Setup & Fixtures
    let server = mcp_server("srv", None, oauth_auth());

    // -- Exec
    let err = validate_login_config(&server)
        .err()
        .ok_or("should reject OAuth server without URL")?;

    // -- Check
    assert!(err.to_string().contains("no URL"), "err={err}");
    Ok(())
}

#[test]
fn test_utility_mcp_validate_login_config_accepts_oauth_with_url() -> Result<()> {
    // -- Setup & Fixtures
    let server = mcp_server("srv", Some("https://x.test/mcp"), oauth_auth());

    // -- Exec
    let (url, auth) = validate_login_config(&server)?;

    // -- Check
    assert_eq!(url, "https://x.test/mcp");
    assert!(matches!(auth, McpAuthConfig::OAuth { .. }));
    Ok(())
}

#[test]
fn test_utility_mcp_list_server_names() {
    // -- Setup & Fixtures
    let config = McpConfig {
        mcp: vec![
            mcp_server("alpha", None, McpAuthConfig::None),
            mcp_server("beta", None, McpAuthConfig::None),
        ],
    };

    // -- Exec
    let names = list_server_names(&config);

    // -- Check
    assert_eq!(names, "alpha, beta");
    assert_eq!(list_server_names(&McpConfig { mcp: vec![] }), "");
}
