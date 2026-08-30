use crate::plugin::AgentPlugin;
use nu_agent_core::config;
use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, LabeledError, Signature, Value};

/// The `agent config init` command generates a starter config.toml from the
/// current environment variables.
pub struct AgentConfigInit;

impl Default for AgentConfigInit {
    fn default() -> Self {
        Self
    }
}

impl SimplePluginCommand for AgentConfigInit {
    type Plugin = AgentPlugin;

    fn name(&self) -> &str {
        "agent config init"
    }

    fn description(&self) -> &str {
        "Generate a starter config.toml from current environment variables"
    }

    fn signature(&self) -> Signature {
        Signature::build(PluginCommand::name(self))
            .switch("force", "Overwrite existing config.toml", None)
            .category(Category::Experimental)
    }

    fn run(
        &self,
        _plugin: &AgentPlugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        let path = config::toml_config::config_path()
            .map_err(|e| LabeledError::new(format!("Cannot determine config path: {e}")))?;

        let force = call
            .has_flag("force")
            .map_err(|e| LabeledError::new(format!("Failed to check --force flag: {e}")))?;
        if path.exists() && !force {
            return Err(LabeledError::new(format!(
                "config.toml already exists at {}. Use --force to overwrite.",
                path.display()
            )));
        }

        let content = generate_config_content();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| LabeledError::new(format!("Failed to create config dir: {e}")))?;
        }
        std::fs::write(&path, &content)
            .map_err(|e| LabeledError::new(format!("Failed to write config: {e}")))?;

        Ok(Value::string(
            format!(
                "Created config.toml at {}\n\nEdit it to set your model, then run:\n  agent provider auth login <provider>  (to store API keys)\n  agent models sync              (to fetch model specs)",
                path.display()
            ),
            call.head,
        ))
    }
}

const TEMPLATE: &str = include_str!("template.toml");

fn generate_config_content() -> String {
    let env_provider = std::env::var("AGENT_PROVIDER").unwrap_or_default();
    let env_model = std::env::var("AGENT_MODEL").unwrap_or_default();

    let active_model = if !env_provider.is_empty() && !env_model.is_empty() {
        format!("[models.default]\nmodel = \"{env_provider}/{env_model}\"")
    } else {
        "# Edit this to set your default model:\n# [models.default]\n# model = \"ollama-cloud/glm-5.2\"".to_string()
    };

    TEMPLATE.replace("{{ACTIVE_MODEL}}", &active_model)
}

#[cfg(test)]
#[path = "init_test.rs"]
mod init_test;
