use crate::tools::limits::truncate_tool_output;
use crate::types::ToolDefinition;
use nu_protocol::{Span, Value};
use rig::tool::{DynamicTool, ToolExecutionError, ToolOutput};
use std::sync::Arc;

use crate::tools::closure::{ClosureRegistry, ResolvedClosure};
use crate::tools::executor::ToolExecutor;
use crate::tools::handler::{json_to_nu_value, nu_value_to_json};

/// Adapts a Nushell closure to rig's DynamicTool interface.
///
/// This adapter bridges our Nushell closure-based tools with rig's dynamic tool system.
/// It wraps a single closure and provides the async interface expected by rig.
pub struct ClosureToolAdapter {
    name: String,
    definition: ToolDefinition,
    resolved: ResolvedClosure,
    executor: Arc<ToolExecutor>,
    span: Span,
    max_tool_result_bytes: usize,
}

impl ClosureToolAdapter {
    /// Create a new adapter for a Nushell closure.
    ///
    /// # Arguments
    ///
    /// * `name` - The tool name to expose to rig
    /// * `resolved` - The resolved closure with pre-extracted parameters
    /// * `executor` - The ToolExecutor for running the closure
    /// * `span` - Span for error reporting
    /// * `max_tool_result_bytes` - Maximum bytes before truncation (0 = disabled)
    pub fn new(
        name: String,
        resolved: ResolvedClosure,
        executor: Arc<ToolExecutor>,
        span: Span,
        max_tool_result_bytes: usize,
    ) -> Self {
        let definition =
            crate::tools::closure::closure_to_tool_definition(name.clone(), &resolved.params, None);

        Self {
            name,
            definition,
            resolved,
            executor,
            span,
            max_tool_result_bytes,
        }
    }

    /// Convert this adapter into a DynamicTool for registration with rig.
    pub fn into_dynamic_tool(self) -> DynamicTool {
        let name = self.name.clone();
        let description = self.definition.description.clone();
        let parameters = self.definition.parameters.clone();
        let resolved = self.resolved;
        let executor = self.executor;
        let span = self.span;
        let max_tool_result_bytes = self.max_tool_result_bytes;

        DynamicTool::new(name, description, parameters, move |_context, args| {
            let resolved = resolved.clone();
            let executor = executor.clone();
            Box::pin(async move {
                // Parse JSON arguments to serde_json::Value
                let args_json: serde_json::Value = serde_json::from_value(args)
                    .map_err(|e| ToolExecutionError::invalid_args(format!("Invalid JSON: {e}")))?;

                // Convert JSON to Nushell Value
                let args_nu_value = json_to_nu_value(&args_json, span).map_err(|e| {
                    ToolExecutionError::invalid_args(format!("JSON to Nu conversion failed: {e}"))
                })?;

                // Extract ordered positional arguments from the record
                let positional_args = if let Value::Record { val, .. } = &args_nu_value {
                    // Use pre-resolved parameter names from the closure
                    let param_names: Vec<&str> =
                        resolved.params.iter().map(|p| p.name.as_str()).collect();

                    // Build positional args in the order expected by the closure
                    param_names
                        .iter()
                        .map(|name| {
                            val.get(name)
                                .cloned()
                                .unwrap_or_else(|| Value::nothing(span))
                        })
                        .collect()
                } else {
                    vec![args_nu_value]
                };

                // Execute the closure via ToolExecutor
                let result = executor
                    .invoke_closure(&resolved.closure, positional_args, span)
                    .await
                    .map_err(|e| ToolExecutionError::provider(format!("{e}")))?;

                // Convert result back to JSON
                let result_json = nu_value_to_json(&result).map_err(|e| {
                    ToolExecutionError::other(format!("Nu to JSON conversion failed: {e}"))
                })?;

                // Serialize to string and cap output size before returning to rig.
                let result_str = serde_json::to_string(&result_json).map_err(|e| {
                    ToolExecutionError::other(format!("JSON serialization failed: {e}"))
                })?;
                Ok(ToolOutput::text(truncate_tool_output(
                    result_str,
                    max_tool_result_bytes,
                )))
            })
        })
    }
}

/// Convert all closures in a registry to DynamicTool instances.
///
/// This function creates a DynamicTool for each closure in the registry,
/// allowing them to be registered with rig's ToolServer.
///
/// # Arguments
///
/// * `registry` - The closure registry containing all tool closures
/// * `executor` - The ToolExecutor for running closures
/// * `span` - Span for error reporting
/// * `max_tool_result_bytes` - Maximum bytes before truncation (0 = disabled)
///
/// # Returns
///
/// A vector of DynamicTool instances, one for each closure in the registry.
/// These can be passed directly to ToolServerHandle::add_dynamic_tool().
pub fn adapt_closures(
    registry: &ClosureRegistry,
    executor: Arc<ToolExecutor>,
    span: Span,
    max_tool_result_bytes: usize,
) -> Vec<DynamicTool> {
    registry
        .names()
        .filter_map(|name| {
            let resolved = registry.get(name)?;
            Some(
                ClosureToolAdapter::new(
                    name.clone(),
                    resolved.clone(),
                    executor.clone(),
                    span,
                    max_tool_result_bytes,
                )
                .into_dynamic_tool(),
            )
        })
        .collect()
}

#[cfg(test)]
#[path = "closure_test.rs"]
mod closure_adapter_test;
