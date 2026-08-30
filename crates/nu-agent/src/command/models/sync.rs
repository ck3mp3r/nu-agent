use nu_agent_core::config::ModelsCache;
use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Signature, Value};

use crate::plugin::AgentPlugin;

pub struct AgentModelsSync;

impl Default for AgentModelsSync {
    fn default() -> Self {
        Self
    }
}

impl SimplePluginCommand for AgentModelsSync {
    type Plugin = AgentPlugin;

    fn name(&self) -> &str {
        "agent models sync"
    }

    fn description(&self) -> &str {
        "Fetch the latest model specs from models.dev and update the local cache"
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Sync the local models cache",
            example: "agent models sync",
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
        let cache = ModelsCache::fetch_and_store()
            .map_err(|e| LabeledError::new(format!("Failed to sync models: {e}")))?;
        let provider_count = cache.providers.len();
        let model_count = cache.list_models(None).len();
        Ok(Value::string(
            format!("Synced {provider_count} providers, {model_count} models"),
            call.head,
        ))
    }
}
