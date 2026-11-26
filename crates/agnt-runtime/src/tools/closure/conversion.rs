use crate::types::{EmptyToolName, ToolAnnotations, ToolDefinition, ToolName};
use nu_plugin::EngineInterface;
use nu_protocol::{Span, Spanned, Value, engine::Closure};
use serde_json::json;

/// The closure contract the tool runtime depends on: read a closure's source
/// span, then evaluate the closure with values.
pub trait ClosureEvaluator: Send + Sync {
    fn get_span_contents(&self, span: Span) -> Result<Vec<u8>, String>;

    /// Evaluate a Nushell closure. Optional: only engines that can run
    /// closures need to override it; the real nu_plugin::EngineInterface does.
    fn eval_closure(
        &self,
        _closure: &Spanned<Closure>,
        _positional: Vec<Value>,
        _input: Option<Value>,
    ) -> Result<Value, String> {
        Err("eval_closure not available for this engine implementation".to_string())
    }

    /// Resolve closure parameters eagerly using the engine's span contents.
    ///
    /// This must be called with the original EngineInterface before cloning,
    /// as cloned engines produce dead contexts where `get_span_contents` fails.
    fn resolve_closure_params(&self, closure: &Spanned<Closure>) -> Vec<ClosureParameter> {
        match self.get_span_contents(closure.span) {
            Ok(bytes) => {
                let source = String::from_utf8_lossy(&bytes).to_string();
                ClosureParameter::parse(&source)
            }
            Err(e) => {
                log::warn!(
                    "Failed to extract closure source for parameter resolution: {}. \
                     Tool will have no parameters.",
                    e
                );
                vec![]
            }
        }
    }
}

/// Implementation for nu_plugin::EngineInterface
impl ClosureEvaluator for EngineInterface {
    fn get_span_contents(&self, span: Span) -> Result<Vec<u8>, String> {
        self.get_span_contents(span)
            .map_err(|e| format!("Failed to get span contents: {e}"))
    }

    fn eval_closure(
        &self,
        closure: &Spanned<Closure>,
        positional: Vec<Value>,
        input: Option<Value>,
    ) -> Result<Value, String> {
        nu_plugin::EngineInterface::eval_closure(self, closure, positional, input)
            .map_err(|e| format!("Closure execution failed: {e}"))
    }
}

/// Engine stand-in for hosts without a Nushell engine (ACP, CLI).
///
/// Such hosts have no span source and no closure evaluation, so the stub
/// reports every capability as unavailable.
#[derive(Default)]
pub struct NoopEvaluator;

impl ClosureEvaluator for NoopEvaluator {
    fn get_span_contents(&self, _span: Span) -> Result<Vec<u8>, String> {
        Err("span contents are not available for this engine".to_string())
    }
}

/// The resolved closure a [`ToolDefinition`] is derived from: its name, its
/// pre-resolved parameters, and an optional description.
///
/// The spec is the local half of the conversion: `ToolDefinition` and `TryFrom`
/// are both foreign, so the tuple the conversion reads has to name a local type.
pub(crate) struct ClosureToolSpec<'a> {
    name: &'a str,
    params: &'a [ClosureParameter],
    description: Option<String>,
}

impl<'a> ClosureToolSpec<'a> {
    /// `name`, its pre-resolved `params`, and an optional `description`.
    pub(crate) fn new(
        name: &'a str,
        params: &'a [ClosureParameter],
        description: Option<String>,
    ) -> Self {
        Self {
            name,
            params,
            description,
        }
    }
}

/// A tool definition derived from a resolved Nushell closure.
///
/// Takes pre-resolved parameters instead of extracting them from an engine,
/// avoiding the dead-context problem with a cloned `EngineInterface`.
///
/// # Errors
///
/// Returns [`EmptyToolName`] when the spec's name is empty.
impl TryFrom<ClosureToolSpec<'_>> for ToolDefinition {
    type Error = EmptyToolName;

    fn try_from(spec: ClosureToolSpec<'_>) -> Result<Self, Self::Error> {
        let ClosureToolSpec {
            name,
            params,
            description,
        } = spec;
        let desc = description.unwrap_or_else(|| format!("Nushell closure tool: {name}"));

        if params.is_empty() {
            return fallback_tool_definition(name, &desc);
        }

        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for param in params {
            properties.insert(
                param.name.clone(),
                json!({
                    "type": "string",
                    "description": format!("Parameter: {}", param.name)
                }),
            );

            if param.is_required {
                required.push(param.name.clone());
            }
        }

        let schema = json!({
            "type": "object",
            "properties": properties,
            "required": required
        });

        Ok(Self {
            annotations: ToolAnnotations::default(),
            name: ToolName::new(name)?,
            description: desc,
            parameters: schema,
        })
    }
}

fn fallback_tool_definition(
    name: &str,
    description: &str,
) -> Result<ToolDefinition, EmptyToolName> {
    let schema = json!({
        "type": "object",
        "properties": {},
        "required": []
    });

    Ok(ToolDefinition {
        annotations: ToolAnnotations::default(),
        name: ToolName::new(name)?,
        description: description.to_string(),
        parameters: schema,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClosureParameter {
    pub name: String,
    pub is_required: bool,
}

impl ClosureParameter {
    /// Parse parameter names from closure source code.
    pub fn parse(source: &str) -> Vec<Self> {
        let source = source.trim();

        let after_brace = if let Some(pos) = source.find('{') {
            source[pos + 1..].trim_start()
        } else {
            return vec![];
        };

        let rest = if let Some(r) = after_brace.strip_prefix('|') {
            r
        } else {
            return vec![];
        };

        let param_str = if let Some(end) = rest.find('|') {
            &rest[..end]
        } else {
            return vec![];
        };

        param_str
            .split(',')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .map(|p| {
                let name_part = p.split(':').next().unwrap_or(p).trim();
                if let Some(name) = name_part.strip_suffix('?') {
                    Self {
                        name: name.trim().to_string(),
                        is_required: false,
                    }
                } else {
                    Self {
                        name: name_part.to_string(),
                        is_required: true,
                    }
                }
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "../../../test/tools/closure/conversion.rs"]
mod conversion_test;
