use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Signature, Value};

use nu_agent_core::config::secrets::{Credential, SecretStore};

use crate::plugin::AgentPlugin;

pub struct AgentProviderAuthStatus;

impl AgentProviderAuthStatus {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AgentProviderAuthStatus {
    fn default() -> Self {
        Self::new()
    }
}

impl SimplePluginCommand for AgentProviderAuthStatus {
    type Plugin = AgentPlugin;

    fn name(&self) -> &str {
        "agent provider auth status"
    }

    fn description(&self) -> &str {
        "Show stored credentials for all LLM providers"
    }

    fn extra_description(&self) -> &str {
        "Displays a table with one row per stored provider credential showing the \
         credential type (api_key or oauth) and, for OAuth, the expiry timestamp."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "auth",
            "status",
            "provider",
            "credentials",
            "list",
            "api-key",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Show stored provider credentials",
            example: "agent provider auth status",
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
        let store = SecretStore::load()
            .map_err(|e| LabeledError::new(format!("Failed to load secret store: {e}")))?;

        let rows: Vec<Value> = store
            .list()
            .into_iter()
            .map(|(name, cred)| {
                let (cred_type, expiry) = match cred {
                    Credential::ApiKey { .. } => ("api_key".to_string(), Value::nothing(call.head)),
                    Credential::OAuth { expires_at, .. } => {
                        let exp = expires_at
                            .map(|t| Value::int(t as i64, call.head))
                            .unwrap_or(Value::nothing(call.head));
                        ("oauth".to_string(), exp)
                    }
                };
                Value::record(
                    nu_protocol::record! {
                        "provider" => Value::string(name, call.head),
                        "type" => Value::string(cred_type, call.head),
                        "expires_at" => expiry,
                    },
                    call.head,
                )
            })
            .collect();

        Ok(Value::list(rows, call.head))
    }
}
