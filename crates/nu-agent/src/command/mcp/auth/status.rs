use std::time::{SystemTime, UNIX_EPOCH};

use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Signature, Value};
use oauth2::TokenResponse;

use nu_agent_core::tools::mcp::config::{McpAuthConfig, McpConfig};
use nu_agent_core::tools::mcp::credentials::{McpCredentialsEntry, McpCredentialsStore};

use crate::plugin::AgentPlugin;

/// Determine the authentication status string for a single MCP server.
///
/// * `auth` — the server's auth configuration
/// * `entry` — optional stored credentials entry (for OAuth servers)
/// * `now` — current unix timestamp in seconds (injected for testability)
pub(crate) fn determine_status(
    auth: &McpAuthConfig,
    entry: Option<&McpCredentialsEntry>,
    now: u64,
) -> String {
    match auth {
        McpAuthConfig::None => "no auth required".to_string(),
        McpAuthConfig::Bearer { .. } => "static token (from config)".to_string(),
        McpAuthConfig::OAuth { .. } => {
            match entry.and_then(|e| e.stored_credentials.as_ref()) {
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
            }
        }
    }
}

pub struct AgentAuthMcpStatus;

impl Default for AgentAuthMcpStatus {
    fn default() -> Self {
        Self
    }
}

impl SimplePluginCommand for AgentAuthMcpStatus {
    type Plugin = AgentPlugin;

    fn name(&self) -> &str {
        "agent mcp auth status"
    }

    fn description(&self) -> &str {
        "Show authentication status for all configured MCP servers"
    }

    fn extra_description(&self) -> &str {
        "Displays a table with one row per configured MCP server showing the \
         auth type and current authentication status. For OAuth servers, the \
         status indicates whether a valid token is stored, an expired token \
         exists (will be refreshed on next use), or login is needed."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["auth", "status", "mcp", "oauth", "credentials", "list"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Show authentication status for all MCP servers",
            example: "agent mcp auth status",
            result: None,
        }]
    }

    fn signature(&self) -> Signature {
        Signature::build(PluginCommand::name(self)).category(Category::Experimental)
    }

    fn run(
        &self,
        _plugin: &AgentPlugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        // 1. Load MCP config
        let plugin_config = nu_agent_core::config::toml_config::load()
            .map_err(|e| LabeledError::new(format!("Failed to load config.toml: {e}")))?;
        let mcp_config = McpConfig::from_toml_config(&plugin_config).map_err(|msg| {
            LabeledError::new("Failed to load MCP config").with_label(msg, call.head)
        })?;

        // 2. Load credential store
        let credential_store = McpCredentialsStore::load()
            .map_err(|e| LabeledError::new(format!("Failed to load credential store: {e}")))?;

        // 3. Build status rows
        let mut rows: Vec<Value> = Vec::new();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        for server in &mcp_config.mcp {
            let auth_type = match &server.auth {
                McpAuthConfig::None => "none".to_string(),
                McpAuthConfig::Bearer { .. } => "bearer".to_string(),
                McpAuthConfig::OAuth { .. } => "oauth".to_string(),
            };

            let entry = credential_store.entries.get(&server.name);
            let status = determine_status(&server.auth, entry, now);

            let record = nu_protocol::record! {
                "server" => Value::string(&server.name, call.head),
                "auth_type" => Value::string(auth_type, call.head),
                "status" => Value::string(status, call.head),
            };

            rows.push(Value::record(record, call.head));
        }

        Ok(Value::list(rows, call.head))
    }
}
