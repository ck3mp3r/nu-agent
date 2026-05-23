use crate::AgentPlugin;
use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Signature, SyntaxShape, Value};

pub struct AgentAuthLogin;

impl AgentAuthLogin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AgentAuthLogin {
    fn default() -> Self {
        Self::new()
    }
}

impl SimplePluginCommand for AgentAuthLogin {
    type Plugin = AgentPlugin;

    fn name(&self) -> &str {
        "agent auth login"
    }

    fn description(&self) -> &str {
        "Authenticate with a provider"
    }

    fn extra_description(&self) -> &str {
        "Runs the OAuth device-code flow. Opens a browser to authenticate with the provider.
Currently supports: github-copilot (default)."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["auth", "login", "oauth", "copilot", "token"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Authenticate with GitHub Copilot (default)",
                example: "agent auth login",
                result: None,
            },
            Example {
                description: "Authenticate with a specific provider",
                example: "agent auth login --provider github-copilot",
                result: None,
            },
        ]
    }

    fn signature(&self) -> Signature {
        Signature::build(PluginCommand::name(self))
            .named(
                "provider",
                SyntaxShape::String,
                "Provider to authenticate with (default: github-copilot)",
                Some('p'),
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
        let provider = call
            .get_flag_value("provider")
            .and_then(|v| v.as_str().ok().map(|s| s.to_string()))
            .unwrap_or_else(|| "github-copilot".to_string());

        match provider.as_str() {
            "github-copilot" | "copilot" => {
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

                // authorize() is async, SimplePluginCommand::run() is sync
                // Create a tokio runtime to execute the async operation
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| LabeledError::new(format!("Failed to create runtime: {e}")))?;

                rt.block_on(client.authorize())
                    .map_err(|e| LabeledError::new(format!("Authentication failed: {e}")))?;

                Ok(Value::string(
                    "Successfully authenticated with GitHub Copilot",
                    call.head,
                ))
            }
            _ => Err(LabeledError::new(format!(
                "Unsupported provider: {provider}. Supported: github-copilot"
            ))),
        }
    }
}
