use std::sync::Arc;

use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, SignalAction, Signature, SyntaxShape, Value};
use tokio::sync::Mutex;

use nu_agent_core::tools::mcp::config::{McpAuthConfig, McpConfig, McpServerConfig};
use nu_agent_core::tools::mcp::credentials::{
    FileCredentialStore, FileStateStore, McpCredentialsStore,
};
use nu_agent_core::tools::mcp::oauth_callback::{CallbackServer, DEFAULT_TIMEOUT_SECS};

use crate::plugin::AgentPlugin;

/// Validate that a server config is suitable for login (OAuth with a URL).
///
/// Returns `(server_url, &McpAuthConfig)` on success, or an error message on failure.
/// The caller should destructure the `McpAuthConfig::OAuth` variant to get the fields.
pub(crate) fn validate_login_config(
    server: &McpServerConfig,
) -> Result<(&str, &McpAuthConfig), String> {
    match &server.auth {
        McpAuthConfig::OAuth { .. } => {}
        _ => {
            return Err(format!(
                "MCP server '{}' does not use OAuth authentication (auth type: {:?})",
                server.name, server.auth
            ));
        }
    }

    let server_url = server.url.as_deref().ok_or_else(|| {
        format!(
            "MCP server '{}' has no URL configured (OAuth requires a server URL)",
            server.name
        )
    })?;

    Ok((server_url, &server.auth))
}

pub struct AgentAuthMcpLogin;

impl Default for AgentAuthMcpLogin {
    fn default() -> Self {
        Self
    }
}

impl SimplePluginCommand for AgentAuthMcpLogin {
    type Plugin = AgentPlugin;

    fn name(&self) -> &str {
        "agent mcp auth login"
    }

    fn description(&self) -> &str {
        "Authenticate with an MCP server via OAuth"
    }

    fn extra_description(&self) -> &str {
        "Runs the OAuth authorization-code flow with PKCE for a configured MCP server. \
         Opens a browser to the authorization URL and waits for the callback. \
         The MCP server must be configured with auth type 'oauth' in the plugin config."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["auth", "login", "mcp", "oauth", "token"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Authenticate with an MCP server via OAuth",
            example: "agent mcp auth login my-server",
            result: None,
        }]
    }

    fn signature(&self) -> Signature {
        Signature::build(PluginCommand::name(self))
            .required(
                "server",
                SyntaxShape::String,
                "MCP server name to authenticate with",
            )
            .category(Category::Experimental)
    }

    fn run(
        &self,
        plugin: &AgentPlugin,
        engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        let server_name: String = call.req(0)?;

        crate::block_on!(plugin, async {
            run_inner(engine, call, &server_name).await
        })
    }
}

async fn run_inner(
    engine: &EngineInterface,
    call: &EvaluatedCall,
    server_name: &str,
) -> Result<Value, LabeledError> {
    // 1. Load MCP config from plugin config
    let plugin_config = nu_agent_core::config::toml_config::load()
        .map_err(|e| LabeledError::new(format!("Failed to load config.toml: {e}")))?;
    let mcp_config = McpConfig::from_toml_config(&plugin_config)
        .map_err(|msg| LabeledError::new("Failed to load MCP config").with_label(msg, call.head))?;

    // 2. Find the server by name
    let server = mcp_config
        .mcp
        .iter()
        .find(|s| s.name == server_name)
        .ok_or_else(|| {
            LabeledError::new(format!("MCP server '{server_name}' not found in config")).with_label(
                format!("Available servers: {}", list_server_names(&mcp_config)),
                call.head,
            )
        })?;

    // 3. Validate the server config for login (OAuth + URL)
    let (server_url, oauth_config) = validate_login_config(server).map_err(LabeledError::new)?;

    let (client_id, client_secret, scope, redirect_uri) = match oauth_config {
        McpAuthConfig::OAuth {
            client_id,
            client_secret,
            scope,
            redirect_uri,
        } => (client_id, client_secret, scope, redirect_uri),
        _ => {
            return Err(
                LabeledError::new("MCP server is not configured for OAuth authentication")
                    .with_label(
                        format!(
                            "Server '{}' has auth type {:?}; expected OAuth",
                            server_name, server.auth
                        ),
                        call.head,
                    ),
            );
        }
    };

    // Validate the server URL for SSRF protection
    nu_agent_core::tools::mcp::safe_http_client::validate_url(server_url).map_err(|e| {
        LabeledError::new(format!("Invalid MCP server URL for '{server_name}': {e}"))
    })?;

    // 5. Load credential store
    let credential_store = McpCredentialsStore::load()
        .map_err(|e| LabeledError::new(format!("Failed to load credential store: {e}")))?;
    let credential_store = Arc::new(Mutex::new(credential_store));

    // 6. Start callback server on a random port
    let callback_server = CallbackServer::start(0)
        .await
        .map_err(|e| LabeledError::new(format!("Failed to start callback server: {e}")))?;
    let actual_port = callback_server.port();

    // 7. Create AuthorizationManager with file-backed stores
    let file_credential_store = FileCredentialStore::new(credential_store.clone(), server_name);
    let file_state_store = FileStateStore::new(credential_store.clone());

    let mut auth_manager = rmcp::transport::AuthorizationManager::new(server_url)
        .await
        .map_err(|e| {
            LabeledError::new(format!(
                "Failed to initialize OAuth for '{server_name}': {e}"
            ))
        })?;

    auth_manager.set_credential_store(file_credential_store);
    auth_manager.set_state_store(file_state_store);

    // 8. Discover OAuth metadata from the server (required before configure_client_id or register_client)
    let metadata = auth_manager.discover_metadata().await.map_err(|e| {
        LabeledError::new(format!(
            "Failed to discover OAuth metadata for '{server_name}': {e}"
        ))
    })?;
    auth_manager.set_metadata(metadata);

    // 9. Configure the OAuth client with the provided client_id and optional client_secret
    if let Some(cid) = client_id {
        let redirect_uri = redirect_uri
            .as_deref()
            .map(|uri| uri.to_string())
            .unwrap_or_else(|| format!("http://127.0.0.1:{actual_port}/mcp/oauth/callback"));
        let mut client_config =
            rmcp::transport::auth::OAuthClientConfig::new(cid.clone(), &redirect_uri);
        if let Some(secret) = client_secret {
            client_config = client_config.with_client_secret(secret.clone());
        }
        auth_manager
            .configure_client(client_config)
            .map_err(|e| LabeledError::new(format!("Failed to configure OAuth client: {e}")))?;
    } else {
        // Dynamic client registration
        let redirect_uri = redirect_uri
            .as_deref()
            .map(|uri| uri.to_string())
            .unwrap_or_else(|| format!("http://127.0.0.1:{actual_port}/mcp/oauth/callback"));
        let scopes: Vec<&str> = scope
            .as_deref()
            .map(|s| s.split(' ').collect())
            .unwrap_or_default();
        let scopes_refs: Vec<&str> = scopes.to_vec();
        auth_manager
            .register_client("nu-agent", &redirect_uri, &scopes_refs)
            .await
            .map_err(|e| LabeledError::new(format!("Dynamic client registration failed: {e}")))?;
    }

    // 10. Get the authorization URL
    let scopes: Vec<&str> = scope
        .as_deref()
        .map(|s| s.split(' ').collect())
        .unwrap_or_default();
    let auth_url = auth_manager
        .get_authorization_url(&scopes)
        .await
        .map_err(|e| LabeledError::new(format!("Failed to generate authorization URL: {e}")))?;

    // 11. Open browser to authorization URL
    eprintln!("Opening browser to authenticate with MCP server '{server_name}'...");
    eprintln!("If the browser doesn't open, visit:\n  {auth_url}");
    eprintln!("\nWaiting for callback on port {actual_port} (timeout: 2 minutes)...");
    eprintln!("Press Ctrl-C to cancel.");

    if let Err(e) = open::that(&auth_url) {
        log::warn!("Failed to open browser: {e}");
        eprintln!("Please open the URL manually in your browser.");
    }

    // 12. Wait for callback (2 min timeout)
    // The CSRF token is embedded in the authorization URL as the `state` query parameter.
    // We parse it from the URL to match against the callback server's pending auth.

    // Parse the CSRF token from the authorization URL
    let parsed_url = url::Url::parse(&auth_url)
        .map_err(|e| LabeledError::new(format!("Failed to parse authorization URL: {e}")))?;

    let csrf_token = parsed_url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.to_string())
        .ok_or_else(|| LabeledError::new("Authorization URL does not contain a state parameter"))?;

    // Register a signal handler for ctrl-c cancellation
    let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
    let cancel_handler: Box<dyn Fn(SignalAction) + Send + Sync> = Box::new(move |action| {
        if matches!(action, SignalAction::Interrupt) {
            let _ = cancel_tx.send(true);
        }
    });
    let _guard = engine
        .register_signal_handler(cancel_handler)
        .map_err(|e| LabeledError::new(format!("Failed to register signal handler: {e}")))?;

    // Wait for the callback, with ctrl-c cancellation
    let auth_code = tokio::select! {
        result = callback_server.wait_for_callback(&csrf_token, DEFAULT_TIMEOUT_SECS) => {
            result.map_err(|e| LabeledError::new(format!("OAuth callback failed for '{server_name}': {e}")))?
        }
        _ = cancel_rx.changed() => {
            let mut callback_server = callback_server;
            callback_server.stop_if_idle();
            return Err(LabeledError::new("Authentication cancelled by user"));
        }
    };

    // 13. Exchange code for tokens
    let _token_response = auth_manager
        .exchange_code_for_token(&auth_code.code, &auth_code.state)
        .await
        .map_err(|e| {
            LabeledError::new(format!(
                "Failed to exchange authorization code for tokens: {e}"
            ))
        })?;

    // 14. Save credentials to disk
    {
        let guard = credential_store.lock().await;
        guard
            .save()
            .map_err(|e| LabeledError::new(format!("Failed to save credentials: {e}")))?;
    }

    // 15. Stop callback server if idle
    // We need a mutable reference to call stop_if_idle
    // callback_server is owned, so we can take it
    let mut callback_server = callback_server;
    callback_server.stop_if_idle();

    eprintln!("Successfully authenticated with MCP server '{server_name}'");
    Ok(Value::string(
        format!("Authenticated with MCP server '{server_name}'"),
        call.head,
    ))
}

fn list_server_names(config: &McpConfig) -> String {
    let names: Vec<&str> = config.mcp.iter().map(|s| s.name.as_str()).collect();
    names.join(", ")
}
