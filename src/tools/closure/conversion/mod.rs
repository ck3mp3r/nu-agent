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
///
/// # Arguments
///
/// * `name` - The name of the tool
/// * `closure` - Spanned reference to the Nushell closure
/// * `engine` - Engine interface to get span contents
/// * `description` - Optional description of what the tool does
///
/// # Returns
///
/// A rig-rs ToolDefinition with named parameters following MCP convention
///
/// # Example
///
/// ```ignore
/// let closure = Spanned {
///     item: Closure { block_id: BlockId::new(0), captures: vec![] },
///     span: Span::new(0, 16),
/// };
/// let tool_def = closure_to_tool_definition(
///     "add".to_string(),
///     &closure,
///     &engine,
///     Some("Add two numbers".to_string()),
/// );
/// // Results in schema with properties: {x: {...}, y: {...}}
/// ```
pub fn closure_to_tool_definition<E: EngineInterfaceLike>(
    name: String,
    closure: &Spanned<Closure>,
    engine: &E,
    description: Option<String>,
) -> ToolDefinition {
    // Use provided description or generate a default
    let desc = description.unwrap_or_else(|| format!("Nushell closure tool: {}", name));

    // Get closure source code from span
    let source = match engine.get_span_contents(closure.span) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
        Err(_) => {
            // Fallback to generic schema if we can't get source
            return fallback_tool_definition(name, desc);
        }
    };

    // Parse parameter names from closure source
    let params = parse_closure_parameters(&source);

    // Build properties and required arrays
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();

    for param in params {
        properties.insert(
            param.name.clone(),
            json!({
                // Omit "type" field to allow LLM type inference.
                // Nushell is dynamically typed, and forcing "string" breaks numeric operations.
                // The LLM will infer the correct type from context (e.g., "5 plus 5" → numbers).
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

/// Fallback when we can't parse closure source
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
///
/// Supports formats like:
/// - `{|| body}` - no parameters
/// - `{|x| body}` - one parameter
/// - `{|x, y| body}` - multiple parameters
/// - `{|x, y?| body}` - optional parameters (marked with ?)
///
/// Returns a list of parameter names in the order they appear, with a flag indicating
/// whether each is required.
pub fn parse_closure_parameters(source: &str) -> Vec<ClosureParameter> {
    let source = source.trim();

    // Find the parameter list between {| and |}
    if let Some(start) = source.find("{|") {
        // Find the closing | after the opening {|
        // We need to skip the first | (at start+1) and find the next one
        if let Some(end_relative) = source[start + 2..].find('|') {
            let param_str = &source[start + 2..start + 2 + end_relative];

            // Split by comma and parse each parameter
            return param_str
                .split(',')
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .map(|p| {
                    // Check if parameter is optional (ends with ?)
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
    }

    // No parameters found
    vec![]
}

/// Extract parameter names from a closure using its source span.
///
/// This is a convenience wrapper around `parse_closure_parameters` that fetches
/// the closure's source code via the engine interface.
///
/// # Arguments
/// * `closure` - The spanned closure
/// * `engine` - Engine interface to get span contents
///
/// # Returns
/// Vector of parameter names in declaration order, or empty vec if parsing fails
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
mod parse_tests;

#[cfg(test)]
mod tests;
