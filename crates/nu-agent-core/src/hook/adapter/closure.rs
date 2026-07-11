use crate::tools::limits::truncate_tool_output;
use crate::types::ToolDefinition;
use nu_protocol::{Span, Value};
use rig::tool::{ToolDyn, ToolError};
use rig::wasm_compat::WasmBoxedFuture;
use std::sync::Arc;

use crate::tools::closure::{ClosureRegistry, ResolvedClosure};
use crate::tools::executor::ToolExecutor;
use crate::tools::handler::{json_to_nu_value, nu_value_to_json};

/// Error type for closure execution failures.
///
/// This error is used to wrap closure execution errors so they can be
/// converted into rig's ToolError::ToolCallError.
#[derive(Debug, thiserror::Error)]
enum ClosureExecError {
    #[error("Tool execution failed: {0}")]
    Execution(String),

    #[error("Argument conversion failed: {0}")]
    ArgumentConversion(String),

    #[error("Result conversion failed: {0}")]
    ResultConversion(String),
}

/// Adapts a Nushell closure to rig's ToolDyn interface.
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
}

impl ToolDyn for ClosureToolAdapter {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn description(&self) -> String {
        self.definition.description.clone()
    }

    fn parameters(&self) -> serde_json::Value {
        self.definition.parameters.clone()
    }

    fn call<'a>(&'a self, args: String) -> WasmBoxedFuture<'a, Result<String, ToolError>> {
        Box::pin(async move {
            // Parse JSON arguments to serde_json::Value
            let args_json: serde_json::Value = serde_json::from_str(&args).map_err(|e| {
                ToolError::ToolCallError(Box::new(ClosureExecError::ArgumentConversion(format!(
                    "Invalid JSON: {e}"
                ))))
            })?;

            // Convert JSON to Nushell Value
            let args_nu_value = json_to_nu_value(&args_json, self.span).map_err(|e| {
                ToolError::ToolCallError(Box::new(ClosureExecError::ArgumentConversion(format!(
                    "JSON to Nu conversion failed: {e}"
                ))))
            })?;

            // Extract ordered positional arguments from the record
            let positional_args = if let Value::Record { val, .. } = &args_nu_value {
                // Use pre-resolved parameter names from the closure
                let param_names: Vec<&str> = self
                    .resolved
                    .params
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect();

                // Build positional args in the order expected by the closure
                param_names
                    .iter()
                    .map(|name| {
                        val.get(name)
                            .cloned()
                            .unwrap_or_else(|| Value::nothing(self.span))
                    })
                    .collect()
            } else {
                vec![args_nu_value]
            };

            // Execute the closure via ToolExecutor
            let result = self
                .executor
                .invoke_closure(&self.resolved.closure, positional_args, self.span)
                .await
                .map_err(|e| {
                    ToolError::ToolCallError(Box::new(ClosureExecError::Execution(format!("{e}"))))
                })?;

            // Convert result back to JSON
            let result_json = nu_value_to_json(&result).map_err(|e| {
                ToolError::ToolCallError(Box::new(ClosureExecError::ResultConversion(format!(
                    "Nu to JSON conversion failed: {e}"
                ))))
            })?;

            // Serialize to string and cap output size before returning to rig.
            let result_str = serde_json::to_string(&result_json).map_err(|e| {
                ToolError::ToolCallError(Box::new(ClosureExecError::ResultConversion(format!(
                    "JSON serialization failed: {e}"
                ))))
            })?;
            Ok(truncate_tool_output(result_str, self.max_tool_result_bytes))
        })
    }
}

/// Convert all closures in a registry to ClosureToolAdapter instances.
///
/// This function creates a ClosureToolAdapter for each closure in the registry,
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
/// A vector of ClosureToolAdapter instances, one for each closure in the registry.
/// These can be passed directly to ToolServerHandle::add_tool() since they implement ToolDyn.
pub fn adapt_closures(
    registry: &ClosureRegistry,
    executor: Arc<ToolExecutor>,
    span: Span,
    max_tool_result_bytes: usize,
) -> Vec<ClosureToolAdapter> {
    registry
        .names()
        .map(|name| {
            let resolved = registry.get(name).unwrap().clone();
            ClosureToolAdapter::new(
                name.clone(),
                resolved,
                executor.clone(),
                span,
                max_tool_result_bytes,
            )
        })
        .collect()
}

#[cfg(test)]
#[path = "closure_test.rs"]
mod closure_adapter_test;
