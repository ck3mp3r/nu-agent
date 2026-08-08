use nu_agent_core::config::ModelsCache;
use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Signature, SyntaxShape, Value};

use crate::plugin::AgentPlugin;

pub struct AgentModelsList;

impl AgentModelsList {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AgentModelsList {
    fn default() -> Self {
        Self::new()
    }
}

impl SimplePluginCommand for AgentModelsList {
    type Plugin = AgentPlugin;

    fn name(&self) -> &str {
        "agent models list"
    }

    fn description(&self) -> &str {
        "List available models from the local cache"
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "List all models in the cache",
            example: "agent models list",
            result: None,
        }]
    }

    fn signature(&self) -> Signature {
        Signature::build(PluginCommand::name(self))
            .named(
                "provider",
                SyntaxShape::String,
                "Filter by provider name",
                None,
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
        let cache = ModelsCache::load().map_err(|e| {
            LabeledError::new(format!(
                "Failed to load models cache: {e}\nRun 'agent models sync' first."
            ))
        })?;
        let provider_filter = call.get_flag::<String>("provider").ok().flatten();
        let models = cache.list_models(provider_filter.as_deref());
        let rows: Vec<Value> = models
            .iter()
            .map(|(provider, model, spec)| {
                Value::record(
                    nu_protocol::record! {
                        "provider" => Value::string(*provider, call.head),
                        "model" => Value::string(*model, call.head),
                        "name" => Value::string(&spec.name, call.head),
                        "context" => Value::int(spec.limit.context as i64, call.head),
                        "output" => Value::int(spec.limit.output as i64, call.head),
                        "tool_call" => Value::bool(spec.tool_call, call.head),
                    },
                    call.head,
                )
            })
            .collect();
        Ok(Value::list(rows, call.head))
    }
}
