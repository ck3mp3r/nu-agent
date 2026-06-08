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
/// Takes pre-resolved parameters instead of extracting them from engine,
/// avoiding the dead-context problem with cloned EngineInterface.
pub fn closure_to_tool_definition(
    name: String,
    params: &[ClosureParameter],
    description: Option<String>,
) -> ToolDefinition {
    let desc = description.unwrap_or_else(|| format!("Nushell closure tool: {}", name));

    if params.is_empty() {
        return fallback_tool_definition(name, desc);
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

#[derive(Debug, Clone, PartialEq)]
pub struct ClosureParameter {
    pub name: String,
    pub is_required: bool,
}

/// Parse parameter names from closure source code.
pub fn parse_closure_parameters(source: &str) -> Vec<ClosureParameter> {
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
                ClosureParameter {
                    name: name.trim().to_string(),
                    is_required: false,
                }
            } else {
                ClosureParameter {
                    name: name_part.to_string(),
                    is_required: true,
                }
            }
        })
        .collect()
}

/// Resolve closure parameters eagerly using the engine's span contents.
///
/// This must be called with the original EngineInterface before cloning,
/// as cloned engines produce dead contexts where `get_span_contents` fails.
pub fn resolve_closure_params<E: EngineInterfaceLike>(
    closure: &Spanned<Closure>,
    engine: &E,
) -> Vec<ClosureParameter> {
    match engine.get_span_contents(closure.span) {
        Ok(bytes) => {
            let source = String::from_utf8_lossy(&bytes).to_string();
            parse_closure_parameters(&source)
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

#[cfg(test)]
#[path = "conversion_test.rs"]
mod conversion_test;
