use nu_plugin::EngineInterface;
use nu_protocol::{Span, Spanned, engine::Closure};
use rig::completion::ToolDefinition;
use serde_json::json;

/// Trait to abstract engine interface for testing
pub trait EngineInterfaceLike {
    fn get_span_contents(&self, span: Span) -> Result<Vec<u8>, String>;
}

/// Implementation for nu_plugin::EngineInterface
impl EngineInterfaceLike for EngineInterface {
    fn get_span_contents(&self, span: Span) -> Result<Vec<u8>, String> {
        self.get_span_contents(span)
            .map_err(|e| format!("Failed to get span contents: {}", e))
    }
}

/// Convert a Nushell closure to a rig-rs ToolDefinition with named parameters.
///
/// Extracts parameter names from the closure's source code by parsing the parameter list.
/// Supports both required and optional parameters (marked with `?`).
pub fn closure_to_tool_definition<E: EngineInterfaceLike>(
    name: String,
    closure: &Spanned<Closure>,
    engine: &E,
    description: Option<String>,
) -> ToolDefinition {
    let desc = description.unwrap_or_else(|| format!("Nushell closure tool: {}", name));

    let source = match engine.get_span_contents(closure.span) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
        Err(_) => {
            return fallback_tool_definition(name, desc);
        }
    };

    let params = parse_closure_parameters(&source);

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
            required.push(param.name);
        }
    }

    let schema = json!({
        "type": "object",
        "properties": properties,
        "required": required
    });

    ToolDefinition {
        name,
        description: desc,
        parameters: schema,
    }
}

fn fallback_tool_definition(name: String, description: String) -> ToolDefinition {
    let schema = json!({
        "type": "object",
        "properties": {},
        "required": []
    });

    ToolDefinition {
        name,
        description,
        parameters: schema,
    }
}

#[derive(Debug, PartialEq)]
pub struct ClosureParameter {
    pub name: String,
    pub is_required: bool,
}

/// Parse parameter names from closure source code.
pub fn parse_closure_parameters(source: &str) -> Vec<ClosureParameter> {
    let source = source.trim();

    if let Some(start) = source.find("{|")
        && let Some(end_relative) = source[start + 2..].find('|')
    {
        let param_str = &source[start + 2..start + 2 + end_relative];

        return param_str
            .split(',')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .map(|p| {
                if let Some(name) = p.strip_suffix('?') {
                    ClosureParameter {
                        name: name.trim().to_string(),
                        is_required: false,
                    }
                } else {
                    ClosureParameter {
                        name: p.to_string(),
                        is_required: true,
                    }
                }
            })
            .collect();
    }

    vec![]
}

/// Extract parameter names from a closure using its source span.
pub fn extract_parameter_names<E: EngineInterfaceLike>(
    closure: &Spanned<Closure>,
    engine: &E,
) -> Vec<String> {
    match engine.get_span_contents(closure.span) {
        Ok(bytes) => {
            let source = String::from_utf8_lossy(&bytes);
            parse_closure_parameters(&source)
                .into_iter()
                .map(|p| p.name)
                .collect()
        }
        Err(_) => vec![],
    }
}

#[cfg(test)]
#[path = "conversion_tests.rs"]
mod conversion_tests;
