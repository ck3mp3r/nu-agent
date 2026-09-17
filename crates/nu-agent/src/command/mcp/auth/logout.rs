use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Signature, SyntaxShape, Value};

use nu_agent_core::config::vault::Vault;

use crate::plugin::AgentPlugin;

/// Perform the logout logic: clear credentials for `server_name` from `vault`.
///
/// Returns a user-facing message indicating success or that no credentials existed.
pub(crate) fn perform_logout(vault: &Vault, server_name: &str) -> String {
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

pub struct AgentAuthMcpLogout;

impl Default for AgentAuthMcpLogout {
    fn default() -> Self {
        Self
    }
}

impl SimplePluginCommand for AgentAuthMcpLogout {
    type Plugin = AgentPlugin;

    fn name(&self) -> &str {
        "agent mcp auth logout"
    }

    fn description(&self) -> &str {
        "Clear stored OAuth credentials for an MCP server"
    }

    fn extra_description(&self) -> &str {
        "Removes any stored OAuth tokens and client registration data for the \
         specified MCP server from the credential store. Does not modify the \
         plugin configuration."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["auth", "logout", "mcp", "oauth", "clear", "credentials"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Clear stored credentials for an MCP server",
            example: "agent mcp auth logout my-server",
            result: None,
        }]
    }

    fn signature(&self) -> Signature {
        Signature::build(PluginCommand::name(self))
            .required(
                "server",
                SyntaxShape::String,
                "MCP server name to clear credentials for",
            )
            .category(Category::Experimental)
    }

    fn run(
        &self,
        _plugin: &AgentPlugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        let server_name: String = call.req(0)?;

        let vault = Vault::auto_detect();

        let msg = perform_logout(&vault, &server_name);

        Ok(Value::string(msg, call.head))
    }
}
