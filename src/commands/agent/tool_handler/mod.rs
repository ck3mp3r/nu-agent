use nu_plugin::EngineInterface;
use nu_protocol::{Span, Value, shell_error::generic::GenericError};
use rig::completion::message::{AssistantContent, ToolCall};
use serde_json::Value as JsonValue;

use crate::tools::{closure::ClosureRegistry, executor::ToolExecutor};

#[derive(Debug, Clone, PartialEq)]
pub enum ToolSource {
    Closure,
    Mcp,
}

#[derive(Debug, Clone)]
pub struct McpToolRegistry {
    names: std::collections::HashSet<String>,
    raw_name_by_exposed_name: std::collections::HashMap<String, String>,
}

impl McpToolRegistry {
    #[cfg(test)]
    pub fn from_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let names: std::collections::HashSet<String> = names.into_iter().map(Into::into).collect();
        Self {
            raw_name_by_exposed_name: names
                .iter()
                .map(|name| (name.clone(), name.clone()))
                .collect(),
            names,
        }
    }

    pub fn from_tools<I>(tools: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = crate::tools::mcp::client::McpToolDefinition>,
    {
        let mut names = std::collections::HashSet::new();
        let mut raw_name_by_exposed_name = std::collections::HashMap::new();

        for tool in tools {
            let exposed_name = tool.name;
            if !names.insert(exposed_name.clone()) {
                return Err(format!(
                    "duplicate exposed MCP tool name '{}' while building MCP registry",
                    exposed_name
                ));
            }
            raw_name_by_exposed_name.insert(exposed_name, tool.raw_name);
        }

        Ok(Self {
            names,
            raw_name_by_exposed_name,
        })
    }

    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    pub fn raw_name_for(&self, exposed_name: &str) -> Option<&str> {
        self.raw_name_by_exposed_name
            .get(exposed_name)
            .map(String::as_str)
    }
}

fn resolve_mcp_invocation_name<'a>(
    registry: &'a McpToolRegistry,
    exposed_tool_name: &str,
) -> Option<&'a str> {
    registry.raw_name_for(exposed_tool_name)
}

fn classify_tool_source(
    tool_name: &str,
    closure_registry: &ClosureRegistry,
    mcp_registry: &McpToolRegistry,
) -> Option<ToolSource> {
    if closure_registry.get(tool_name).is_some() {
        Some(ToolSource::Closure)
    } else if mcp_registry.contains(tool_name) {
        Some(ToolSource::Mcp)
    } else {
        None
    }
}

/// Convert a serde_json::Value to nu_protocol::Value.
///
/// Recursively converts JSON values to their Nushell equivalents.
///
/// # Arguments
/// * `json` - The JSON value to convert
/// * `span` - The span for error reporting and value creation
///
/// # Returns
/// A Nushell Value, or ShellError if conversion fails
#[allow(clippy::result_large_err)]
pub fn json_to_nu_value(json: &JsonValue, span: Span) -> Result<Value, GenericError> {
    match json {
        JsonValue::Null => Ok(Value::nothing(span)),
        JsonValue::Bool(b) => Ok(Value::bool(*b, span)),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::int(i, span))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::float(f, span))
            } else {
                Err(GenericError::new(
                    "Invalid JSON number",
                    "Could not convert number",
                    span,
                ))
            }
        }
        JsonValue::String(s) => Ok(Value::string(s.clone(), span)),
        JsonValue::Array(arr) => {
            let values: Result<Vec<Value>, GenericError> = arr
                .iter()
                .map(|item| json_to_nu_value(item, span))
                .collect();
            Ok(Value::list(values?, span))
        }
        JsonValue::Object(obj) => {
            let mut record = nu_protocol::record!();
            for (key, value) in obj {
                record.insert(key.clone(), json_to_nu_value(value, span)?);
            }
            Ok(Value::record(record, span))
        }
    }
}

/// Convert a nu_protocol::Value to serde_json::Value.
///
/// Recursively converts Nushell values to their JSON equivalents.
///
/// # Arguments
/// * `value` - The Nushell value to convert
///
/// # Returns
/// A JSON value, or ShellError if conversion fails
#[allow(clippy::result_large_err)]
pub fn nu_value_to_json(value: &Value) -> Result<JsonValue, GenericError> {
    match value {
        Value::Nothing { .. } => Ok(JsonValue::Null),
        Value::Bool { val, .. } => Ok(JsonValue::Bool(*val)),
        Value::Int { val, .. } => Ok(JsonValue::Number((*val).into())),
        Value::Float { val, .. } => serde_json::Number::from_f64(*val)
            .map(JsonValue::Number)
            .ok_or_else(|| {
                GenericError::new(
                    "Invalid float value",
                    "Cannot convert float to JSON",
                    value.span(),
                )
            }),
        Value::String { val, .. } => Ok(JsonValue::String(val.clone())),
        Value::List { vals, .. } => {
            let json_values: Result<Vec<JsonValue>, GenericError> =
                vals.iter().map(nu_value_to_json).collect();
            Ok(JsonValue::Array(json_values?))
        }
        Value::Record { val, .. } => {
            let mut map = serde_json::Map::new();
            for (key, value) in val.iter() {
                map.insert(key.clone(), nu_value_to_json(value)?);
            }
            Ok(JsonValue::Object(map))
        }
        _ => Err(GenericError::new(
            "Unsupported value type",
            format!("Cannot convert {:?} to JSON", value),
            value.span(),
        )),
    }
}

/// Result of executing a single tool call.
///
/// Contains the tool call ID and the serialized JSON result.
#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: String,
    pub source: ToolSource,
    pub content: String,
}

/// Handle multiple tool calls from LLM response.
///
/// Executes each tool call sequentially and returns a list of results.
///
/// # Arguments
/// * `tool_calls` - List of AssistantContent items that may contain tool calls
/// * `closure_registry` - Registry to look up tool closures by name
/// * `tool_executor` - Executor for running the closures
/// * `engine` - Engine interface for extracting closure parameter names
/// * `span` - Span for error reporting
///
/// # Returns
/// Vector of ToolCallResult, one for each successful tool call execution
pub async fn handle_tool_calls(
    tool_calls: Vec<AssistantContent>,
    closure_registry: &ClosureRegistry,
    mcp_registry: &McpToolRegistry,
    mcp_tool_server: Option<&rig::tool::server::ToolServerHandle>,
    tool_executor: &ToolExecutor,
    engine: &EngineInterface,
    span: Span,
) -> Result<Vec<ToolCallResult>, GenericError> {
    let mut results = Vec::new();

    for content in tool_calls {
        // Only process ToolCall variants
        if let AssistantContent::ToolCall(tool_call) = content {
            let result = handle_single_tool_call(
                tool_call,
                closure_registry,
                mcp_registry,
                mcp_tool_server,
                tool_executor,
                engine,
                span,
            )
            .await?;

            results.push(result);
        }
    }

    Ok(results)
}

/// Handle a single tool call.
///
/// Looks up the tool closure, parses arguments, executes it, and returns the result.
/// Arguments are extracted by parameter name and passed to the closure in the correct order.
///
/// # Arguments
/// * `tool_call` - The ToolCall from rig-rs containing id, function (with name and arguments)
/// * `closure_registry` - Registry to look up tool closures by name
/// * `tool_executor` - Executor for running the closures
/// * `engine` - Engine interface for extracting closure parameter names
/// * `span` - Span for error reporting
///
/// # Returns
/// ToolCallResult with the tool call ID and JSON-serialized result
async fn handle_single_tool_call(
    tool_call: ToolCall,
    closure_registry: &ClosureRegistry,
    mcp_registry: &McpToolRegistry,
    mcp_tool_server: Option<&rig::tool::server::ToolServerHandle>,
    tool_executor: &ToolExecutor,
    engine: &EngineInterface,
    span: Span,
) -> Result<ToolCallResult, GenericError> {
    // Look up closure by function name
    let serialized_arguments =
        serde_json::to_string(&tool_call.function.arguments).unwrap_or_else(|_| "{}".to_string());

    let source = if let Some(source) =
        classify_tool_source(&tool_call.function.name, closure_registry, mcp_registry)
    {
        source
    } else {
        return Err(GenericError::new(
            format!("Tool '{}' not found", tool_call.function.name),
            "Unknown tool",
            span,
        ));
    };

    if source == ToolSource::Mcp {
        let server = mcp_tool_server.ok_or_else(|| {
            GenericError::new(
                "MCP runtime unavailable",
                "MCP tool server handle is not initialized",
                span,
            )
        })?;

        let raw_tool_name =
            resolve_mcp_invocation_name(mcp_registry, &tool_call.function.name).ok_or_else(|| {
            GenericError::new(
                format!(
                    "MCP tool '{}' is registered but missing raw-name mapping",
                    tool_call.function.name
                ),
                "MCP execution error",
                span,
            )
            })?;

        let content = server
            .call_tool(raw_tool_name, &serialized_arguments)
            .await
            .map_err(|e| {
                GenericError::new(
                    format!("MCP tool execution failed: {e}"),
                    "MCP execution error",
                    span,
                )
            })?;

        return Ok(ToolCallResult {
            tool_call_id: tool_call.id,
            tool_name: tool_call.function.name,
            arguments: serialized_arguments,
            source,
            content,
        });
    }

    let closure = closure_registry
        .get(&tool_call.function.name)
        .ok_or_else(|| {
            GenericError::new(
                format!("Tool '{}' not found", tool_call.function.name),
                "Unknown tool",
                span,
            )
        })?;

    // Parse arguments from JSON Value
    let args_json = json_to_nu_value(&tool_call.function.arguments, span)?;

    // Extract positional arguments by matching parameter names
    let positional_args = if let Value::Record { val, .. } = &args_json {
        // Get parameter names from closure source
        use crate::tools::closure::extract_parameter_names;
        let param_names = extract_parameter_names(closure, engine);

        // Extract values in parameter order
        param_names
            .iter()
            .map(|name| {
                val.get(name)
                    .cloned()
                    .unwrap_or_else(|| Value::nothing(span))
            })
            .collect()
    } else {
        // Not a record - pass as single argument (fallback for compatibility)
        vec![args_json]
    };

    // Execute closure via ToolExecutor (closure is already Spanned)
    let result = tool_executor
        .invoke_closure(closure, positional_args, span)
        .await
        .map_err(|e| {
            GenericError::new(
                format!("Tool execution failed: {}", e),
                "Execution error",
                span,
            )
        })?;

    // Convert result back to JSON string
    let result_json = nu_value_to_json(&result)?;
    let content = serde_json::to_string(&result_json).map_err(|e| {
        GenericError::new(
            format!("Result serialization failed: {}", e),
            "JSON error",
            span,
        )
    })?;

    Ok(ToolCallResult {
        tool_call_id: tool_call.id,
        tool_name: tool_call.function.name,
        arguments: serialized_arguments,
        source,
        content,
    })
}

#[cfg(test)]
mod tests;
