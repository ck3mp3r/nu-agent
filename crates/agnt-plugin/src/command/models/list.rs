use agnt_core::config::{ModelSpec, ModelsCache};
use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Signature, Span, SyntaxShape, Value};

use crate::plugin::AgentPlugin;

pub struct AgentModelsList;

impl Default for AgentModelsList {
    fn default() -> Self {
        Self
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
        let paths = agnt_core::config::Paths::resolve_from_config_or_default()
            .map_err(|e| LabeledError::new(format!("Failed to resolve application paths: {e}")))?;
        let cache = ModelsCache::load_from(&paths.models_cache_path).map_err(|e| {
            LabeledError::new(format!(
                "Failed to load models cache: {e}\nRun 'agent models sync' first."
            ))
        })?;
        let provider_filter = call.get_flag::<String>("provider").ok().flatten();
        let models = cache.list_models(provider_filter.as_deref());
        let rows: Vec<Value> = models
            .iter()
            .map(|(provider, model, spec)| model_record(provider, model, spec, call.head))
            .collect();
        Ok(Value::list(rows, call.head))
    }
}

// region:    --- Functions

/// Build the record for one cached model.
fn model_record(provider: &str, model: &str, spec: &ModelSpec, span: Span) -> Value {
    let (input_cost, output_cost) = match &spec.cost {
        Some(cost) => (
            Value::float(cost.input, span),
            Value::float(cost.output, span),
        ),
        None => (Value::nothing(span), Value::nothing(span)),
    };
    Value::record(
        nu_protocol::record! {
            "provider" => Value::string(provider, span),
            "model" => Value::string(model, span),
            "name" => Value::string(&spec.name, span),
            "context" => Value::int(spec.limit.context as i64, span),
            "output" => Value::int(spec.limit.output as i64, span),
            "tool_call" => Value::bool(spec.tool_call, span),
            "input_cost" => input_cost,
            "output_cost" => output_cost,
        },
        span,
    )
}

// endregion: --- Functions

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/command/models/list.rs"]
mod list_test;

// endregion: --- Tests
