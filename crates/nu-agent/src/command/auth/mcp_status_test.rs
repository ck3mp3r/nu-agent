use oauth2::{AccessToken, basic::BasicTokenType};
use rmcp::transport::auth::{OAuthTokenResponse, StoredCredentials, VendorExtraTokenFields};

use nu_agent_core::tools::mcp::config::McpAuthConfig;
use nu_agent_core::tools::mcp::credentials::McpCredentialsEntry;

use super::mcp_status::determine_status;

fn make_entry_with_token(
    expires_in_secs: Option<u64>,
    received_at: Option<u64>,
) -> McpCredentialsEntry {
    let mut resp = OAuthTokenResponse::new(
        AccessToken::new("test-token".to_string()),
        BasicTokenType::Bearer,
        VendorExtraTokenFields::default(),
    );
    if let Some(expires_in) = expires_in_secs {
        resp.set_expires_in(Some(std::time::Duration::from_secs(expires_in)).as_ref());
    }

    McpCredentialsEntry {
        stored_credentials: Some(StoredCredentials::new(
            "test-client".to_string(),
            Some(resp),
            vec![],
            received_at,
        )),
        ..Default::default()
    }
}

fn make_entry_without_token() -> McpCredentialsEntry {
    McpCredentialsEntry {
        stored_credentials: Some(StoredCredentials::new(
            "test-client".to_string(),
            None,
            vec![],
            None,
        )),
        ..Default::default()
    }
}

#[test]
fn status_none_auth_shows_no_auth_required() {
    let auth = McpAuthConfig::None;
    let status = determine_status(&auth, None, 0);
    assert_eq!(status, "no auth required");
}

#[test]
fn status_bearer_auth_shows_static_token() {
    let auth = McpAuthConfig::Bearer {
        token: "my-token".to_string(),
    };
    let status = determine_status(&auth, None, 0);
    assert_eq!(status, "static token (from config)");
}

#[test]
fn status_oauth_with_valid_token_shows_authenticated() {
    let auth = McpAuthConfig::OAuth {
        client_id: Some("cid".to_string()),
        client_secret: None,
        scope: None,
        redirect_uri: None,
    };
    // Token received at t=100, expires in 60s → expires at t=160
    // now=150 → not expired
    let entry = make_entry_with_token(Some(60), Some(100));
    let status = determine_status(&auth, Some(&entry), 150);
    assert_eq!(status, "authenticated (token valid)");
}

#[test]
fn status_oauth_with_expired_token_shows_will_refresh() {
    let auth = McpAuthConfig::OAuth {
        client_id: Some("cid".to_string()),
        client_secret: None,
        scope: None,
        redirect_uri: None,
    };
    // Token received at t=100, expires in 60s → expires at t=160
    // now=200 → expired
    let entry = make_entry_with_token(Some(60), Some(100));
    let status = determine_status(&auth, Some(&entry), 200);
    assert_eq!(status, "authenticated (token expired — will refresh)");
}

#[test]
fn status_oauth_without_credentials_shows_not_authenticated() {
    let auth = McpAuthConfig::OAuth {
        client_id: Some("cid".to_string()),
        client_secret: None,
        scope: None,
        redirect_uri: None,
    };
    let status = determine_status(&auth, None, 0);
    assert_eq!(
        status,
        "not authenticated (run: agent mcp auth login <name>)"
    );
}

#[test]
fn status_oauth_with_entry_but_no_token_response_shows_not_authenticated() {
    let auth = McpAuthConfig::OAuth {
        client_id: Some("cid".to_string()),
        client_secret: None,
        scope: None,
        redirect_uri: None,
    };
    let entry = make_entry_without_token();
    let status = determine_status(&auth, Some(&entry), 0);
    assert_eq!(
        status,
        "not authenticated (run: agent mcp auth login <name>)"
    );
}

#[test]
fn status_oauth_with_token_no_expiry_info_shows_valid() {
    let auth = McpAuthConfig::OAuth {
        client_id: Some("cid".to_string()),
        client_secret: None,
        scope: None,
        redirect_uri: None,
    };
    // Token with no expires_in and no received_at → assume valid
    let entry = make_entry_with_token(None, None);
    let status = determine_status(&auth, Some(&entry), 999999);
    assert_eq!(status, "authenticated (token valid)");
}
