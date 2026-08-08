use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Signature, SyntaxShape, Value};

use nu_agent_core::config::secrets::SecretStore;

use crate::plugin::AgentPlugin;

pub struct AgentProviderAuthLogout;

impl AgentProviderAuthLogout {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AgentProviderAuthLogout {
    fn default() -> Self {
        Self::new()
    }
}

impl SimplePluginCommand for AgentProviderAuthLogout {
    type Plugin = AgentPlugin;

    fn name(&self) -> &str {
        "agent provider auth logout"
    }

    fn description(&self) -> &str {
        "Clear stored credentials for an LLM provider"
    }

    fn extra_description(&self) -> &str {
        "Removes the stored credential entry for the specified provider from the \
         nu-agent secret store ($XDG_DATA_HOME/nu-agent/secrets.json)."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "auth",
            "logout",
            "provider",
            "credentials",
            "clear",
            "api-key",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Clear stored credentials for a provider",
            example: "agent provider auth logout openai",
            result: None,
        }]
    }

    fn signature(&self) -> Signature {
        Signature::build(PluginCommand::name(self))
            .required(
                "name",
                SyntaxShape::String,
                "Provider name to clear credentials for",
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
        let name: String = call.req(0)?;

        let mut store = SecretStore::load()
            .map_err(|e| LabeledError::new(format!("Failed to load secret store: {e}")))?;

        match store.remove(&name) {
            Some(_) => {
                store
                    .save()
                    .map_err(|e| LabeledError::new(format!("Failed to save secret store: {e}")))?;
                Ok(Value::string(
                    format!("Removed credentials for '{name}'"),
                    call.head,
                ))
            }
            None => Ok(Value::string(
                format!("No stored credentials for '{name}'"),
                call.head,
            )),
        }
    }
}
