use nu_plugin::EngineInterface;
use nu_protocol::{Span, Value, shell_error::generic::GenericError};
use rig::completion::message::{AssistantContent, ToolCall};
use serde_json::Value as JsonValue;

use crate::tools::{closure::ClosureRegistry, error::ToolError, executor::ToolExecutor};

#[derive(Debug, serde::Deserialize)]
struct BuiltinReadArgs {
    path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
struct BuiltinEditArgs {
    path: String,
    search: String,
    replacement: String,
    #[serde(default)]
    expected_version: Option<String>,
    #[serde(default)]
    match_mode: Option<String>,
    #[serde(default)]
    occurrence: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct BuiltinPatchRangeArgs {
    start: usize,
    end: usize,
}

#[derive(Debug, serde::Deserialize)]
struct BuiltinPatchOpArgs {
    range: BuiltinPatchRangeArgs,
    replacement: String,
}

#[derive(Debug, serde::Deserialize)]
struct BuiltinPatchArgs {
    path: String,
    #[serde(default)]
    expected_version: Option<String>,
    operations: Vec<BuiltinPatchOpArgs>,
}

#[derive(Debug, Clone)]
struct BuiltinFsToolError {
    kind: ToolErrorKind,
    message: String,
    details: Option<JsonValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolSource {
    Closure,
    Mcp,
    Unknown,
}

impl ToolSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Closure => "closure",
            Self::Mcp => "mcp",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolErrorKind {
    Timeout,
    Validation,
    Runtime,
    Transport,
    Unknown,
}

impl ToolErrorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Validation => "validation",
            Self::Runtime => "runtime",
            Self::Transport => "transport",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolFailureOutcome {
    pub tool_name: String,
    pub tool_call_id: String,
    pub source: ToolSource,
    pub error_kind: ToolErrorKind,
    pub message: String,
    pub details: Option<JsonValue>,
}

impl ToolFailureOutcome {
    pub fn to_json_value(&self) -> JsonValue {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "tool_name".to_string(),
            JsonValue::String(self.tool_name.clone()),
        );
        obj.insert(
            "tool_call_id".to_string(),
            JsonValue::String(self.tool_call_id.clone()),
        );
        obj.insert(
            "source".to_string(),
            JsonValue::String(self.source.as_str().to_string()),
        );
        obj.insert(
            "error_kind".to_string(),
            JsonValue::String(self.error_kind.as_str().to_string()),
        );
        obj.insert(
            "message".to_string(),
            JsonValue::String(self.message.clone()),
        );

        if let Some(details) = &self.details {
            obj.insert("details".to_string(), details.clone());
        }

        JsonValue::Object(obj)
    }

    fn to_json_string(&self) -> String {
        serde_json::to_string(&self.to_json_value()).unwrap_or_else(|_| {
            format!(
                r#"{{"tool_name":"{}","tool_call_id":"{}","source":"{}","error_kind":"{}","message":"{}"}}"#,
                self.tool_name,
                self.tool_call_id,
                self.source.as_str(),
                self.error_kind.as_str(),
                self.message
            )
        })
    }
}

#[derive(Debug, Clone)]
pub struct McpToolRegistry {
    names: std::collections::HashSet<String>,
    raw_name_by_exposed_name: std::collections::HashMap<String, String>,
    server_by_exposed_name: std::collections::HashMap<String, String>,
    enabled_servers: std::sync::Arc<std::sync::RwLock<std::collections::HashSet<String>>>,
}

impl McpToolRegistry {
    #[cfg(test)]
    pub fn from_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let names: std::collections::HashSet<String> = names.into_iter().map(Into::into).collect();
        let raw_name_by_exposed_name = names
            .iter()
            .map(|name| (name.clone(), name.clone()))
            .collect::<std::collections::HashMap<_, _>>();
        let server_by_exposed_name = names
            .iter()
            .map(|name| (name.clone(), name.clone()))
            .collect::<std::collections::HashMap<_, _>>();
        let enabled_servers = names.iter().cloned().collect();
        Self {
            raw_name_by_exposed_name,
            server_by_exposed_name,
            enabled_servers: std::sync::Arc::new(std::sync::RwLock::new(enabled_servers)),
            names,
        }
    }

    pub fn from_tools<I>(tools: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = crate::tools::mcp::client::McpToolDefinition>,
    {
        let mut names = std::collections::HashSet::new();
        let mut raw_name_by_exposed_name = std::collections::HashMap::new();
        let mut server_by_exposed_name = std::collections::HashMap::new();
        let mut enabled_servers = std::collections::HashSet::new();

        for tool in tools {
            let exposed_name = tool.name;
            let raw_name = tool.raw_name;
            let server_name = tool.server;
            if !names.insert(exposed_name.clone()) {
                return Err(format!(
                    "duplicate exposed MCP tool name '{}' while building MCP registry",
                    exposed_name
                ));
            }
            raw_name_by_exposed_name.insert(exposed_name.clone(), raw_name);
            server_by_exposed_name.insert(exposed_name, server_name.clone());
            enabled_servers.insert(server_name);
        }

        Ok(Self {
            names,
            raw_name_by_exposed_name,
            server_by_exposed_name,
            enabled_servers: std::sync::Arc::new(std::sync::RwLock::new(enabled_servers)),
        })
    }

    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name) && self.is_tool_enabled(name)
    }

    pub fn is_registered(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    pub fn raw_name_for(&self, exposed_name: &str) -> Option<&str> {
        self.raw_name_by_exposed_name
            .get(exposed_name)
            .map(String::as_str)
    }

    pub fn is_tool_enabled(&self, exposed_name: &str) -> bool {
        let Some(server_name) = self.server_by_exposed_name.get(exposed_name) else {
            return false;
        };

        self.enabled_servers
            .read()
            .map(|servers| servers.contains(server_name))
            .unwrap_or(false)
    }

    pub fn set_server_enabled(&self, server_name: &str, enabled: bool) -> Result<(), String> {
        let mut servers = self
            .enabled_servers
            .write()
            .map_err(|_| "MCP enabled-server state lock poisoned".to_string())?;

        if enabled {
            servers.insert(server_name.to_string());
        } else {
            servers.remove(server_name);
        }

        Ok(())
    }

    pub fn is_server_enabled(&self, server_name: &str) -> bool {
        self.enabled_servers
            .read()
            .map(|servers| servers.contains(server_name))
            .unwrap_or(false)
    }
}

fn resolve_mcp_invocation_name<'a>(
    registry: &'a McpToolRegistry,
    exposed_tool_name: &str,
) -> Option<&'a str> {
    registry.raw_name_for(exposed_tool_name)
}

pub(crate) fn llm_visible_tool_definitions(
    tool_definitions: &[rig::completion::ToolDefinition],
    mcp_registry: &McpToolRegistry,
) -> Vec<rig::completion::ToolDefinition> {
    tool_definitions
        .iter()
        .filter(|tool| {
            if mcp_registry.is_registered(tool.name.as_str()) {
                mcp_registry.contains(tool.name.as_str())
            } else {
                true
            }
        })
        .cloned()
        .collect()
}

fn classify_tool_source(
    tool_name: &str,
    closure_registry: &ClosureRegistry,
    mcp_registry: &McpToolRegistry,
) -> Option<ToolSource> {
    if closure_registry.get(tool_name).is_some() || is_builtin_fs_tool_name(tool_name) {
        Some(ToolSource::Closure)
    } else if mcp_registry.contains(tool_name) {
        Some(ToolSource::Mcp)
    } else {
        None
    }
}

pub(crate) fn is_builtin_fs_tool_name(tool_name: &str) -> bool {
    matches!(tool_name, "read" | "edit" | "patch")
}

fn parse_edit_match_mode(value: Option<&str>) -> Result<crate::tools::fs::core::EditMatchMode, BuiltinFsToolError> {
    match value.unwrap_or("literal") {
        "literal" => Ok(crate::tools::fs::core::EditMatchMode::Literal),
        "regex" => Ok(crate::tools::fs::core::EditMatchMode::Regex),
        other => Err(BuiltinFsToolError {
            kind: ToolErrorKind::Validation,
            message: format!("Invalid edit.match_mode '{other}': expected 'literal' or 'regex'"),
            details: None,
        }),
    }
}

fn parse_edit_occurrence(value: Option<&str>) -> Result<crate::tools::fs::core::EditOccurrence, BuiltinFsToolError> {
    match value.unwrap_or("first") {
        "first" => Ok(crate::tools::fs::core::EditOccurrence::First),
        "all" => Ok(crate::tools::fs::core::EditOccurrence::All),
        other => Err(BuiltinFsToolError {
            kind: ToolErrorKind::Validation,
            message: format!("Invalid edit.occurrence '{other}': expected 'first' or 'all'"),
            details: None,
        }),
    }
}

fn map_mutate_error(error: crate::tools::fs::core::MutateError) -> BuiltinFsToolError {
    use crate::tools::fs::core::MutateError;

    match error {
        MutateError::Io(io_error) => BuiltinFsToolError {
            kind: ToolErrorKind::Runtime,
            message: io_error.to_string(),
            details: None,
        },
        other => BuiltinFsToolError {
            kind: ToolErrorKind::Validation,
            message: other.to_string(),
            details: None,
        },
    }
}

fn resolve_builtin_fs_path_for_cwd(path: &str, cwd: &std::path::Path) -> std::path::PathBuf {
    let raw = std::path::Path::new(path);
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        cwd.join(raw)
    }
}

fn resolve_builtin_fs_path(
    path: &str,
    engine: &EngineInterface,
) -> Result<std::path::PathBuf, BuiltinFsToolError> {
    let cwd = engine.get_current_dir().map_err(|e| BuiltinFsToolError {
        kind: ToolErrorKind::Runtime,
        message: format!("Unable to resolve current working directory: {e}"),
        details: None,
    })?;
    Ok(resolve_builtin_fs_path_for_cwd(path, std::path::Path::new(&cwd)))
}

fn dispatch_builtin_fs_tool(
    tool_name: &str,
    arguments: &JsonValue,
    cwd: &std::path::Path,
) -> Result<Option<JsonValue>, BuiltinFsToolError> {
    use crate::tools::fs::core::{
        EditOperation, PatchOp, PatchRange, ReadRequest, apply_line_range_patch_batch,
        apply_search_replace_edit, read_file,
    };

    match tool_name {
        "read" => {
            let args: BuiltinReadArgs =
                serde_json::from_value(arguments.clone()).map_err(|e| BuiltinFsToolError {
                    kind: ToolErrorKind::Validation,
                    message: format!("Invalid read arguments: {e}"),
                    details: None,
                })?;

            let resolved_path = resolve_builtin_fs_path_for_cwd(&args.path, cwd);
            let response = read_file(
                &resolved_path,
                ReadRequest {
                    offset: args.offset,
                    limit: args.limit,
                },
            )
            .map_err(|e| BuiltinFsToolError {
                kind: ToolErrorKind::Runtime,
                message: format!("read failed: {e}"),
                details: None,
            })?;

            Ok(Some(serde_json::json!({
                "path": args.path,
                "content": response.content,
                "total_lines": response.total_lines,
                "offset": response.offset,
                "limit": response.limit,
                "version": response.version,
            })))
        }
        "edit" => {
            let args: BuiltinEditArgs =
                serde_json::from_value(arguments.clone()).map_err(|e| BuiltinFsToolError {
                    kind: ToolErrorKind::Validation,
                    message: format!("Invalid edit arguments: {e}"),
                    details: None,
                })?;

            let resolved_path = resolve_builtin_fs_path_for_cwd(&args.path, cwd);
            let match_mode = parse_edit_match_mode(args.match_mode.as_deref())?;
            let occurrence = parse_edit_occurrence(args.occurrence.as_deref())?;

            let summary = apply_search_replace_edit(
                &resolved_path,
                args.expected_version.as_deref(),
                &EditOperation {
                    search: args.search,
                    replacement: args.replacement,
                    match_mode,
                    occurrence,
                },
            )
            .map_err(map_mutate_error)?;

            Ok(Some(serde_json::json!({
                "path": args.path,
                "replacements": summary.replacements,
                "wrote": summary.wrote,
                "changed": summary.changed,
                "noop": summary.noop,
                "conflict": summary.conflict,
                "expected_version": summary.expected_version,
                "previous_version": summary.previous_version,
                "new_version": summary.new_version,
            })))
        }
        "patch" => {
            let args: BuiltinPatchArgs =
                serde_json::from_value(arguments.clone()).map_err(|e| BuiltinFsToolError {
                    kind: ToolErrorKind::Validation,
                    message: format!("Invalid patch arguments: {e}"),
                    details: None,
                })?;

            let resolved_path = resolve_builtin_fs_path_for_cwd(&args.path, cwd);
            let operations = args
                .operations
                .into_iter()
                .map(|op| PatchOp {
                    range: PatchRange::new(op.range.start, op.range.end),
                    replacement: op.replacement,
                })
                .collect::<Vec<_>>();

            let summary = apply_line_range_patch_batch(
                &resolved_path,
                args.expected_version.as_deref(),
                operations,
            )
            .map_err(map_mutate_error)?;

            Ok(Some(serde_json::json!({
                "path": args.path,
                "operation_count": summary.operation_count,
                "applied_ranges": summary
                    .applied_ranges
                    .iter()
                    .map(|range| serde_json::json!({"start": range.start, "end": range.end}))
                    .collect::<Vec<_>>(),
                "wrote": summary.wrote,
                "changed": summary.changed,
                "noop": summary.noop,
                "conflict": summary.conflict,
                "expected_version": summary.expected_version,
                "previous_version": summary.previous_version,
                "new_version": summary.new_version,
                "previous_lines": summary.previous_lines,
                "new_lines": summary.new_lines,
            })))
        }
        _ => Ok(None),
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
    pub failure: Option<ToolFailureOutcome>,
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
) -> Vec<ToolCallResult> {
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
            .await;

            results.push(result);
        }
    }

    results
}

fn classify_validation_error_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("invalid")
        || lower.contains("must be")
        || lower.contains("expected")
        || lower.contains("missing")
        || lower.contains("parse")
}

fn build_failure_result(
    tool_call: &ToolCall,
    source: ToolSource,
    error_kind: ToolErrorKind,
    message: String,
    details: Option<JsonValue>,
) -> ToolCallResult {
    let serialized_arguments =
        serde_json::to_string(&tool_call.function.arguments).unwrap_or_else(|_| "{}".to_string());

    let failure = ToolFailureOutcome {
        tool_name: tool_call.function.name.clone(),
        tool_call_id: tool_call.id.clone(),
        source: source.clone(),
        error_kind,
        message,
        details,
    };

    ToolCallResult {
        tool_call_id: tool_call.id.clone(),
        tool_name: tool_call.function.name.clone(),
        arguments: serialized_arguments,
        source,
        content: failure.to_json_string(),
        failure: Some(failure),
    }
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
) -> ToolCallResult {
    // Look up closure by function name
    let serialized_arguments =
        serde_json::to_string(&tool_call.function.arguments).unwrap_or_else(|_| "{}".to_string());

    let source = if let Some(source) =
        classify_tool_source(&tool_call.function.name, closure_registry, mcp_registry)
    {
        source
    } else {
        return build_failure_result(
            &tool_call,
            ToolSource::Unknown,
            ToolErrorKind::Unknown,
            format!("Tool '{}' not found", tool_call.function.name),
            None,
        );
    };

    if source == ToolSource::Mcp {
        let Some(server) = mcp_tool_server else {
            return build_failure_result(
                &tool_call,
                ToolSource::Mcp,
                ToolErrorKind::Transport,
                "MCP runtime unavailable: MCP tool server handle is not initialized".to_string(),
                None,
            );
        };

        let raw_tool_name = if let Some(name) =
            resolve_mcp_invocation_name(mcp_registry, &tool_call.function.name)
        {
            name
        } else {
            return build_failure_result(
                &tool_call,
                ToolSource::Mcp,
                ToolErrorKind::Runtime,
                format!(
                    "MCP tool '{}' is registered but missing raw-name mapping",
                    tool_call.function.name
                ),
                None,
            );
        };

        let content = match server.call_tool(raw_tool_name, &serialized_arguments).await {
            Ok(content) => content,
            Err(e) => {
                return build_failure_result(
                    &tool_call,
                    ToolSource::Mcp,
                    ToolErrorKind::Transport,
                    format!("MCP tool execution failed: {e}"),
                    None,
                );
            }
        };

        return ToolCallResult {
            tool_call_id: tool_call.id,
            tool_name: tool_call.function.name,
            arguments: serialized_arguments,
            source,
            content,
            failure: None,
        };
    }

    let builtin_cwd = match resolve_builtin_fs_path(".", engine) {
        Ok(path) => path,
        Err(err) => {
            return build_failure_result(
                &tool_call,
                ToolSource::Closure,
                err.kind,
                format!("Tool execution failed: {}", err.message),
                err.details,
            );
        }
    };

    match dispatch_builtin_fs_tool(&tool_call.function.name, &tool_call.function.arguments, &builtin_cwd) {
        Ok(Some(payload)) => {
            let content = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
            return ToolCallResult {
                tool_call_id: tool_call.id,
                tool_name: tool_call.function.name,
                arguments: serialized_arguments,
                source,
                content,
                failure: None,
            };
        }
        Ok(None) => {}
        Err(err) => {
            return build_failure_result(
                &tool_call,
                ToolSource::Closure,
                err.kind,
                format!("Tool execution failed: {}", err.message),
                err.details,
            );
        }
    }

    let Some(closure) = closure_registry.get(&tool_call.function.name) else {
        return build_failure_result(
            &tool_call,
            ToolSource::Closure,
            ToolErrorKind::Unknown,
            format!("Tool '{}' not found", tool_call.function.name),
            None,
        );
    };

    // Parse arguments from JSON Value
    let args_json = match json_to_nu_value(&tool_call.function.arguments, span) {
        Ok(v) => v,
        Err(e) => {
            return build_failure_result(
                &tool_call,
                ToolSource::Closure,
                ToolErrorKind::Validation,
                format!("Invalid tool arguments: {e}"),
                None,
            );
        }
    };

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
        .await;

    let result = match result {
        Ok(result) => result,
        Err(ToolError::Timeout { tool_name, timeout }) => {
            return build_failure_result(
                &tool_call,
                ToolSource::Closure,
                ToolErrorKind::Timeout,
                format!("Tool '{}' timed out after {:?}", tool_name, timeout),
                Some(serde_json::json!({ "timeout_ms": timeout.as_millis() })),
            );
        }
        Err(ToolError::Execution(err)) => {
            let msg = err.to_string();
            let kind = if classify_validation_error_message(&msg) {
                ToolErrorKind::Validation
            } else {
                ToolErrorKind::Runtime
            };

            return build_failure_result(
                &tool_call,
                ToolSource::Closure,
                kind,
                format!("Tool execution failed: {msg}"),
                None,
            );
        }
        Err(ToolError::Audit(err)) => {
            return build_failure_result(
                &tool_call,
                ToolSource::Closure,
                ToolErrorKind::Runtime,
                format!("Tool audit failed: {err}"),
                None,
            );
        }
    };

    // Convert result back to JSON string
    let result_json = match nu_value_to_json(&result) {
        Ok(v) => v,
        Err(e) => {
            return build_failure_result(
                &tool_call,
                ToolSource::Closure,
                ToolErrorKind::Runtime,
                format!("Result conversion failed: {e}"),
                None,
            );
        }
    };
    let content = match serde_json::to_string(&result_json) {
        Ok(content) => content,
        Err(e) => {
            return build_failure_result(
                &tool_call,
                ToolSource::Closure,
                ToolErrorKind::Runtime,
                format!("Result serialization failed: {e}"),
                None,
            );
        }
    };

    ToolCallResult {
        tool_call_id: tool_call.id,
        tool_name: tool_call.function.name,
        arguments: serialized_arguments,
        source,
        content,
        failure: None,
    }
}

#[cfg(test)]
mod tests;
