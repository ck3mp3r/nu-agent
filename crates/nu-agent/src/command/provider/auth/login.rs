use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Signature, SyntaxShape, Value};

use nu_agent_core::config::secrets::{Credential, SecretStore};

use crate::plugin::AgentPlugin;

pub struct AgentProviderAuthLogin;

impl Default for AgentProviderAuthLogin {
    fn default() -> Self {
        Self
    }
}

impl SimplePluginCommand for AgentProviderAuthLogin {
    type Plugin = AgentPlugin;

    fn name(&self) -> &str {
        "agent provider auth login"
    }

    fn description(&self) -> &str {
        "Authenticate with an LLM provider"
    }

    fn extra_description(&self) -> &str {
        "Stores credentials in the nu-agent secret store. For github-copilot, runs the OAuth \
         device-code flow. For other providers, accepts an API key via --api-key or piped \
         from stdin. Keys are persisted to $XDG_DATA_HOME/nu-agent/secrets.json."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["auth", "login", "provider", "api-key", "copilot", "oauth"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Authenticate with a provider using an API key",
                example: "agent provider auth login openai --api-key sk-...",
                result: None,
            },
            Example {
                description: "Authenticate with GitHub Copilot via device-code flow",
                example: "agent provider auth login github-copilot",
                result: None,
            },
            Example {
                description: "Authenticate with a provider by piping the API key",
                example: "echo \"sk-...\" | agent provider auth login openai",
                result: None,
            },
        ]
    }

    fn signature(&self) -> Signature {
        Signature::build(PluginCommand::name(self))
            .required(
                "name",
                SyntaxShape::String,
                "Provider name (e.g. 'openai', 'anthropic', 'github-copilot')",
            )
            .named(
                "api-key",
                SyntaxShape::String,
                "API key to store (alternatively, pipe via stdin: echo \"sk-...\" | agent provider auth login <name>)",
                None,
            )
            .category(Category::Experimental)
    }

    fn run(
        &self,
        plugin: &AgentPlugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        input: &Value,
    ) -> Result<Value, LabeledError> {
        let name: String = call.req(0)?;

        // github-copilot uses the OAuth device-code flow.
        if name == "github-copilot" || name == "copilot" {
            crate::block_on!(plugin, async {
                let client = rig::providers::copilot::Client::builder()
                    .oauth()
                    .on_device_code(|params| {
                        eprintln!(
                            "Sign in with GitHub Copilot:\n  1) Visit: {}\n  2) Enter code: {}",
                            params.verification_uri, params.user_code
                        );
                    })
                    .build()
                    .map_err(|e| LabeledError::new(format!("Authentication failed: {e}")))?;

                client
                    .authorize()
                    .await
                    .map_err(|e| LabeledError::new(format!("Authentication failed: {e}")))?;

                Ok(Value::string(
                    "Successfully authenticated with GitHub Copilot",
                    call.head,
                ))
            })
        } else {
            // API key from --api-key flag, or piped via stdin
            let key = if let Some(k) = call
                .get_flag::<String>("api-key")
                .map_err(|e| LabeledError::new(format!("Failed to get --api-key flag: {e}")))?
            {
                k
            } else if let Ok(k) = input.as_str() {
                k.to_string()
            } else {
                return Err(LabeledError::new("API key required").with_label(
                    "Use --api-key <key> or pipe the key: echo \"sk-...\" | agent provider auth login <name>",
                    call.head,
                ));
            };

            let mut store = SecretStore::load()
                .map_err(|e| LabeledError::new(format!("Failed to load secret store: {e}")))?;
            store.set(name.clone(), Credential::ApiKey { key });
            store
                .save()
                .map_err(|e| LabeledError::new(format!("Failed to save secret store: {e}")))?;

            Ok(Value::string(
                format!("Stored API key for '{name}'"),
                call.head,
            ))
        }
    }
}
