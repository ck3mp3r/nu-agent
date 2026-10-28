//! `agent mcp` subcommand dispatch.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use agnt_core::config::vault::Vault;
use agnt_core::tools::mcp::config::{McpAuthConfig, McpConfig, McpServerConfig};
use agnt_core::tools::mcp::oauth_callback::{CallbackServer, DEFAULT_TIMEOUT_SECS};
use oauth2::TokenResponse;
use rmcp::transport::auth::StoredCredentials;

use crate::McpAuthCommand;
use crate::McpCommand;
use crate::McpLoginArgs;
use crate::McpLogoutArgs;
use crate::error::{Error, Result};

use super::table::print_table;

// region:    --- Functions

/// Dispatch an `agent mcp` subcommand.
pub async fn run_mcp_command(command: &McpCommand) -> Result<()> {
    match command {
        McpCommand::Auth(auth) => run_mcp_auth_command(auth).await,
    }
}

async fn run_mcp_auth_command(command: &McpAuthCommand) -> Result<()> {
    match command {
        McpAuthCommand::Login(args) => run_mcp_login(args).await,
        McpAuthCommand::Logout(args) => run_mcp_logout(args),
        McpAuthCommand::Status => run_mcp_status(),
    }
}

/// Authenticate with an MCP server via OAuth.
async fn run_mcp_login(args: &McpLoginArgs) -> Result<()> {
    let server_name = args.server.as_str();

    let plugin_config = agnt_core::config::toml_config::load()
        .map_err(|e| Error::Custom(format!("Failed to load config.toml: {e}")))?;
    let mcp_config = McpConfig::from_toml_config(&plugin_config)
        .map_err(|msg| Error::Custom(format!("Failed to load MCP config: {msg}")))?;

    let server = mcp_config
        .mcp
        .iter()
        .find(|s| s.name == server_name)
        .ok_or_else(|| {
            Error::Custom(format!(
                "MCP server '{server_name}' not found in config. Available servers: {}",
                list_server_names(&mcp_config)
            ))
        })?;

    let (server_url, oauth_config) = validate_login_config(server)?;

    let (client_id, client_secret, scope, redirect_uri) = match oauth_config {
        McpAuthConfig::OAuth {
            client_id,
            client_secret,
            scope,
            redirect_uri,
        } => (client_id, client_secret, scope, redirect_uri),
        _ => {
            return Err(Error::Custom(format!(
                "MCP server '{server_name}' is not configured for OAuth authentication"
            )));
        }
    };

    agnt_core::tools::mcp::safe_http_client::validate_url(server_url)
        .map_err(|e| Error::Custom(format!("Invalid MCP server URL for '{server_name}': {e}")))?;

    let application_paths = agnt_core::config::Paths::resolve_from_config_or_default()
        .map_err(|e| Error::Custom(format!("Failed to resolve application paths: {e}")))?;
    let vault = Arc::new(
        Vault::auto_detect(&application_paths.secrets_path)
            .map_err(|e| Error::Custom(e.to_string()))?,
    );

    let callback_server = CallbackServer::start(0)
        .await
        .map_err(|e| Error::Custom(format!("Failed to start callback server: {e}")))?;
    let actual_port = callback_server.port();

    let vault_credential_store = vault.mcp_credential_store(server_name);
    let vault_state_store = vault.mcp_state_store();

    let mut auth_manager = rmcp::transport::AuthorizationManager::new(server_url)
        .await
        .map_err(|e| {
            Error::Custom(format!(
                "Failed to initialize OAuth for '{server_name}': {e}"
            ))
        })?;

    auth_manager.set_credential_store(vault_credential_store);
    auth_manager.set_state_store(vault_state_store);

    let resolution = auth_manager.resolve_metadata().await.map_err(|e| {
        Error::Custom(format!(
            "Failed to discover OAuth metadata for '{server_name}': {e}"
        ))
    })?;
    auth_manager.set_metadata(resolution.metadata);

    let redirect_uri = redirect_uri
        .as_deref()
        .map(str::to_string)
        .unwrap_or_else(|| format!("http://127.0.0.1:{actual_port}/mcp/oauth/callback"));

    if let Some(client_id) = client_id {
        let mut client_config =
            rmcp::transport::auth::OAuthClientConfig::new(client_id.clone(), &redirect_uri);
        if let Some(client_secret) = client_secret {
            client_config = client_config.with_client_secret(client_secret.clone());
        }
        auth_manager
            .configure_client(client_config)
            .map_err(|e| Error::Custom(format!("Failed to configure OAuth client: {e}")))?;
    } else {
        let scopes: Vec<&str> = scope
            .as_deref()
            .map(|s| s.split(' ').collect())
            .unwrap_or_default();
        auth_manager
            .register_client("nu-agent", &redirect_uri, &scopes)
            .await
            .map_err(|e| Error::Custom(format!("Dynamic client registration failed: {e}")))?;
    }

    let scopes: Vec<&str> = scope
        .as_deref()
        .map(|s| s.split(' ').collect())
        .unwrap_or_default();
    let auth_url = auth_manager
        .get_authorization_url(&scopes)
        .await
        .map_err(|e| Error::Custom(format!("Failed to generate authorization URL: {e}")))?;

    eprintln!("Opening browser to authenticate with MCP server '{server_name}'...");
    eprintln!("If the browser doesn't open, visit:\n  {auth_url}");
    eprintln!("\nWaiting for callback on port {actual_port} (timeout: 2 minutes)...");
    eprintln!("Press Ctrl-C to cancel.");

    if let Err(e) = open::that(&auth_url) {
        log::warn!("Failed to open browser: {e}");
        eprintln!("Please open the URL manually in your browser.");
    }

    // The CSRF token is embedded in the authorization URL as the `state` query
    // parameter. Parse it from the URL to match the callback server's pending auth.
    let parsed_url = url::Url::parse(&auth_url)
        .map_err(|e| Error::Custom(format!("Failed to parse authorization URL: {e}")))?;
    let csrf_token = parsed_url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.to_string())
        .ok_or_else(|| {
            Error::Custom("Authorization URL does not contain a state parameter".to_string())
        })?;

    let auth_code = tokio::select! {
        result = callback_server.wait_for_callback(&csrf_token, DEFAULT_TIMEOUT_SECS) => {
            result.map_err(|e| {
                Error::Custom(format!("OAuth callback failed for '{server_name}': {e}"))
            })?
        }
        _ = tokio::signal::ctrl_c() => {
            let mut callback_server = callback_server;
            callback_server.stop_if_idle();
            return Err(Error::Custom("Authentication cancelled by user".to_string()));
        }
    };

    auth_manager
        .exchange_code_for_token(&auth_code.code, &auth_code.state)
        .await
        .map_err(|e| {
            Error::Custom(format!(
                "Failed to exchange authorization code for tokens: {e}"
            ))
        })?;

    // The vault credential adapter persisted the tokens during the exchange.
    let mut callback_server = callback_server;
    callback_server.stop_if_idle();

    println!("Authenticated with MCP server '{server_name}'");
    Ok(())
}

/// Clear stored OAuth credentials for an MCP server.
fn run_mcp_logout(args: &McpLogoutArgs) -> Result<()> {
    let application_paths = agnt_core::config::Paths::resolve_from_config_or_default()
        .map_err(|e| Error::Custom(format!("Failed to resolve application paths: {e}")))?;
    let vault = Vault::auto_detect(&application_paths.secrets_path)
        .map_err(|e| Error::Custom(e.to_string()))?;
    println!("{}", perform_logout(&vault, &args.server));
    Ok(())
}

/// Show authentication status for all configured MCP servers.
fn run_mcp_status() -> Result<()> {
    let plugin_config = agnt_core::config::toml_config::load()
        .map_err(|e| Error::Custom(format!("Failed to load config.toml: {e}")))?;
    let mcp_config = McpConfig::from_toml_config(&plugin_config)
        .map_err(|msg| Error::Custom(format!("Failed to load MCP config: {msg}")))?;

    if mcp_config.mcp.is_empty() {
        println!("No MCP servers configured.");
        return Ok(());
    }

    let application_paths = agnt_core::config::Paths::resolve_from_config_or_default()
        .map_err(|e| Error::Custom(format!("Failed to resolve application paths: {e}")))?;
    let vault = Vault::auto_detect(&application_paths.secrets_path)
        .map_err(|e| Error::Custom(e.to_string()))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let rows: Vec<Vec<String>> = mcp_config
        .mcp
        .iter()
        .map(|server| {
            let auth_type = match &server.auth {
                McpAuthConfig::None => "none",
                McpAuthConfig::Bearer { .. } => "bearer",
                McpAuthConfig::OAuth { .. } => "oauth",
            };
            let creds = vault.get_mcp_credentials(&server.name).unwrap_or(None);
            let status = determine_status(&server.auth, creds.as_ref(), now);
            vec![server.name.clone(), auth_type.to_string(), status]
        })
        .collect();

    print_table(&["SERVER", "AUTH TYPE", "STATUS"], &rows);
    Ok(())
}

// endregion: --- Functions

// region:    --- Support

/// Validate that a server config is suitable for login (OAuth with a URL).
///
/// Returns `(server_url, &McpAuthConfig)` on success, or an error message on failure.
fn validate_login_config(server: &McpServerConfig) -> Result<(&str, &McpAuthConfig)> {
    if !matches!(server.auth, McpAuthConfig::OAuth { .. }) {
        return Err(Error::Custom(format!(
            "MCP server '{}' does not use OAuth authentication (auth type: {:?})",
            server.name, server.auth
        )));
    }

    let server_url = server.url.as_deref().ok_or_else(|| {
        Error::Custom(format!(
            "MCP server '{}' has no URL configured (OAuth requires a server URL)",
            server.name
        ))
    })?;

    Ok((server_url, &server.auth))
}

/// Clear credentials for `server_name` from `vault`.
///
/// Returns a user-facing message indicating success or that no credentials existed.
fn perform_logout(vault: &Vault, server_name: &str) -> String {
    if vault
        .get_mcp_credentials(server_name)
        .unwrap_or(None)
        .is_none()
    {
        return format!("No stored credentials for '{server_name}'");
    }
    match vault.clear_mcp_credentials(server_name) {
        Ok(()) => format!("Cleared credentials for '{server_name}'"),
        Err(e) => format!("Failed to clear credentials for '{server_name}': {e}"),
    }
}

/// Determine the authentication status string for a single MCP server.
///
/// * `auth` — the server's auth configuration
/// * `creds` — optional stored credentials (for OAuth servers)
/// * `now` — current unix timestamp in seconds
fn determine_status(auth: &McpAuthConfig, creds: Option<&StoredCredentials>, now: u64) -> String {
    match auth {
        McpAuthConfig::None => "no auth required".to_string(),
        McpAuthConfig::Bearer { .. } => "static token (from config)".to_string(),
        McpAuthConfig::OAuth { .. } => match creds {
            Some(creds) if creds.token_response.is_some() => {
                let is_expired = if let Some(token_response) = creds.token_response.as_ref()
                    && let Some(expires_in) = token_response.expires_in()
                    && let Some(received_at) = creds.token_received_at
                {
                    let expires_at = received_at + expires_in.as_secs();
                    now >= expires_at
                } else {
                    false // No expiry info — assume valid
                };

                if is_expired {
                    "authenticated (token expired — will refresh)".to_string()
                } else {
                    "authenticated (token valid)".to_string()
                }
            }
            _ => "not authenticated (run: agent mcp auth login <name>)".to_string(),
        },
    }
}

fn list_server_names(config: &McpConfig) -> String {
    let names: Vec<&str> = config.mcp.iter().map(|s| s.name.as_str()).collect();
    names.join(", ")
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/dispatch/utility/mcp.rs"]
mod mcp_test;

// endregion: --- Tests
