use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Signature, SyntaxShape, Value};

use nu_agent_core::tools::mcp::credentials::McpCredentialsStore;

use crate::plugin::AgentPlugin;

/// Perform the logout logic: remove credentials for `server_name` from `store`.
///
/// Returns a user-facing message indicating success or that no credentials existed.
pub(crate) fn perform_logout(store: &mut McpCredentialsStore, server_name: &str) -> String {
    if store.entries.contains_key(server_name) {
        store.remove(server_name);
        format!("Cleared credentials for '{server_name}'")
    } else {
        format!("No stored credentials for '{server_name}'")
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

        let mut store = McpCredentialsStore::load()
            .map_err(|e| LabeledError::new(format!("Failed to load credential store: {e}")))?;

        let msg = perform_logout(&mut store, &server_name);

        store
            .save()
            .map_err(|e| LabeledError::new(format!("Failed to save credential store: {e}")))?;

        Ok(Value::string(msg, call.head))
    }
}
