use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Signature, SyntaxShape, Value};

use agnt_core::config::copilot_tokens::read_copilot_token_record;
use agnt_core::config::vault::Vault;
use agnt_core::types::default_token_dir;

use crate::plugin::AgentPlugin;

pub struct AgentProviderAuthLogout;

impl Default for AgentProviderAuthLogout {
    fn default() -> Self {
        Self
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
         nu-agent vault (OS keychain, or the file store when no keychain is available)."
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

        // github-copilot stores its tokens in files, not the vault.
        if (name == "github-copilot" || name == "copilot")
            && let Some(dir) = default_token_dir()
            && read_copilot_token_record(&dir)
                .map_err(LabeledError::new)?
                .is_some()
        {
            remove_copilot_token_files(&dir).map_err(LabeledError::new)?;
            return Ok(Value::string(
                format!("Removed credentials for '{name}'"),
                call.head,
            ));
        }

        let vault = Vault::auto_detect().map_err(|e| LabeledError::new(e.to_string()))?;

        let removed = vault
            .remove_provider(&name)
            .map_err(|e| LabeledError::new(format!("Failed to remove credentials: {e}")))?;

        match removed {
            true => Ok(Value::string(
                format!("Removed credentials for '{name}'"),
                call.head,
            )),
            false => Ok(Value::string(
                format!("No stored credentials for '{name}'"),
                call.head,
            )),
        }
    }
}

// region:    --- Support

/// Delete the two Copilot token files, leaving the directory in place.
fn remove_copilot_token_files(dir: &std::path::Path) -> Result<(), String> {
    for file in ["access-token", "api-key.json"] {
        let path = dir.join(file);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Failed to remove {}: {e}", path.display())),
        }
    }
    Ok(())
}

// endregion: --- Support
