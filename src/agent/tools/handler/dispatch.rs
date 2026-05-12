use nu_protocol::Value;
use rig::completion::message::{AssistantContent, ToolCall};

use crate::agent::tools::authz::{AskApprovalHook, PermissionEventSink};
use crate::tools::{closure::ClosureRegistry, error::ToolError};

use super::{
    McpToolRegistry, ToolCallResult, ToolErrorKind, ToolHandlerContext, ToolSource,
    build_authorization_denied_result, build_direct_tool_display, build_failure_result,
    classify_validation_error_message, is_builtin_fs_tool_name, json_to_nu_value, nu_value_to_json,
};

pub(crate) fn resolve_mcp_invocation_name<'a>(
    registry: &'a McpToolRegistry,
    exposed_tool_name: &str,
) -> Option<&'a str> {
    registry.raw_name_for(exposed_tool_name)
}

pub(crate) fn classify_tool_source(
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

pub async fn handle_tool_calls(
    tool_calls: Vec<AssistantContent>,
    context: &mut ToolHandlerContext<'_, impl AskApprovalHook, impl PermissionEventSink>,
) -> Vec<ToolCallResult> {
    let mut results = Vec::new();

    for content in tool_calls {
        if let AssistantContent::ToolCall(tool_call) = content {
            let result = handle_single_tool_call(tool_call, context).await;
            results.push(result);
        }
    }

    results
}

async fn handle_single_tool_call(
    tool_call: ToolCall,
    context: &mut ToolHandlerContext<'_, impl AskApprovalHook, impl PermissionEventSink>,
) -> ToolCallResult {
    let serialized_arguments =
        serde_json::to_string(&tool_call.function.arguments).unwrap_or_else(|_| "{}".to_string());

    let source = if let Some(source) = classify_tool_source(
        &tool_call.function.name,
        context.closure_registry,
        context.mcp_registry,
    ) {
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

    let pre_authorize_output =
        super::pre_authorize::pre_authorize_tool_call(&tool_call, source.clone(), context.engine);
    let flow_context = super::authz_gate::AuthorizationFlowContext {
        ask_context: pre_authorize_output.ask_context.clone(),
        denied_display: pre_authorize_output.display.clone(),
    };

    if let Some(denied_details) = super::authz_gate::enforce_authorization_for_tool_call(
        &tool_call,
        source.clone(),
        context.authorization.permissions,
        context.authorization.grant_cache,
        &flow_context,
        context.authorization.ask_hook,
        context.authorization.event_sink,
    ) {
        return build_authorization_denied_result(
            &tool_call,
            source,
            denied_details,
            flow_context.denied_display.clone(),
        );
    }

    if source == ToolSource::Mcp {
        let Some(server) = context.mcp_tool_server else {
            return build_failure_result(
                &tool_call,
                ToolSource::Mcp,
                ToolErrorKind::Transport,
                "MCP runtime unavailable: MCP tool server handle is not initialized".to_string(),
                None,
            );
        };

        let raw_tool_name = if let Some(name) =
            resolve_mcp_invocation_name(context.mcp_registry, &tool_call.function.name)
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
            display: None,
            failure: None,
        };
    }

    let builtin_cwd = match super::builtin_fs::resolve_builtin_fs_path(".", context.engine) {
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

    match super::builtin_fs::dispatch_builtin_fs_tool(
        &tool_call.function.name,
        &tool_call.function.arguments,
        &builtin_cwd,
    ) {
        Ok(Some(payload)) => {
            let display = build_direct_tool_display(&tool_call.function.name, &payload);
            let content = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
            return ToolCallResult {
                tool_call_id: tool_call.id,
                tool_name: tool_call.function.name,
                arguments: serialized_arguments,
                source,
                content,
                display,
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

    let Some(closure) = context.closure_registry.get(&tool_call.function.name) else {
        return build_failure_result(
            &tool_call,
            ToolSource::Closure,
            ToolErrorKind::Unknown,
            format!("Tool '{}' not found", tool_call.function.name),
            None,
        );
    };

    let args_json = match json_to_nu_value(&tool_call.function.arguments, context.span) {
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

    let positional_args = if let Value::Record { val, .. } = &args_json {
        use crate::tools::closure::extract_parameter_names;
        let param_names = extract_parameter_names(closure, context.engine);

        param_names
            .iter()
            .map(|name| {
                val.get(name)
                    .cloned()
                    .unwrap_or_else(|| Value::nothing(context.span))
            })
            .collect()
    } else {
        vec![args_json]
    };

    let result = context
        .tool_executor
        .invoke_closure(closure, positional_args, context.span)
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
        display: None,
        failure: None,
    }
}
