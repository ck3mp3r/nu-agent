use super::audit::{AuditEntry, AuditLogger, AuditResult};
use super::error::ToolError;
use chrono::Utc;
use nu_plugin::EngineInterface;
use nu_protocol::shell_error::generic::GenericError;
use nu_protocol::{Span, Spanned, Value, engine::Closure};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::timeout as tokio_timeout;

pub struct ToolExecutor {
    engine: Arc<EngineInterface>,
    audit_logger: Arc<AuditLogger>,
    timeout: Duration,
}

impl ToolExecutor {
    pub fn new(
        engine: Arc<EngineInterface>,
        audit_logger: Arc<AuditLogger>,
        timeout: Duration,
    ) -> Self {
        Self {
            engine,
            audit_logger,
            timeout,
        }
    }

    /// Invoke a Nushell closure with timeout enforcement.
    ///
    /// This method executes a Nushell closure with the provided arguments,
    /// enforcing a timeout to prevent long-running or hanging operations.
    ///
    /// # Arguments
    ///
    /// * `closure` - The Nushell closure to execute (must be Spanned)
    /// * `args` - The positional arguments to pass to the closure (in order)
    /// * `span` - The span for error reporting
    ///
    /// # Returns
    ///
    /// * `Ok(Value)` - The result of the closure execution
    /// * `Err(ToolError::Timeout)` - If execution exceeds the configured timeout
    /// * `Err(ToolError::Execution)` - If the closure execution fails
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Example closure: {|x, y| $x + $y }
    /// let spanned_closure = Spanned {
    ///     item: closure,
    ///     span,
    /// };
    /// let result = executor.invoke_closure(
    ///     &spanned_closure,
    ///     vec![Value::int(5, span), Value::int(3, span)],
    ///     span
    /// ).await?;
    /// // result is Value::int(8, span)
    /// ```
    #[allow(clippy::result_large_err)] // ShellError is from nu_protocol, can't change size
    pub async fn invoke_closure(
        &self,
        closure: &Spanned<Closure>,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, ToolError> {
        let start = Instant::now();
        // Best-effort tool name for error messages and audit logging
        let tool_name = format!("{:?}", closure.item);

        // Execute closure with timeout enforcement
        // Note: eval_closure is synchronous, so we spawn_blocking to avoid blocking the async runtime
        let engine = Arc::clone(&self.engine);
        let closure_clone = closure.clone();
        let args_clone = args.clone();

        let result = tokio_timeout(self.timeout, async move {
            tokio::task::spawn_blocking(move || {
                // Arguments are already in correct positional order
                let positional = args_clone;
                let input = None;

                // Call EngineInterface.eval_closure()
                // This evaluates the closure in the plugin's engine context
                engine.eval_closure(&closure_clone, positional, input)
            })
            .await
        })
        .await;

        let duration = start.elapsed();

        // Handle the nested Result from timeout, spawn_blocking, and eval_closure
        // and prepare both the return value and audit result
        let (audit_result, return_value) = match result {
            // Timeout succeeded, spawn_blocking succeeded, eval_closure succeeded
            Ok(Ok(Ok(value))) => {
                let audit = AuditResult::Ok(
                    serde_json::to_value(&value).unwrap_or(serde_json::Value::Null),
                );
                (audit, Ok(value))
            }
            // Timeout succeeded, spawn_blocking succeeded, eval_closure failed
            Ok(Ok(Err(e))) => {
                let audit = AuditResult::Err(format!("Execution error: {}", e));
                (audit, Err(ToolError::Execution(e)))
            }
            // Timeout succeeded, spawn_blocking failed (task panic)
            Ok(Err(join_error)) => {
                let err_msg = format!("Closure execution panicked: {}", join_error);
                let audit = AuditResult::Err(err_msg.clone());
                let shell_error =
                    GenericError::new("Closure execution panicked", join_error.to_string(), span)
                        .into();
                (audit, Err(ToolError::Execution(shell_error)))
            }
            // Timeout elapsed
            Err(_elapsed) => {
                let audit = AuditResult::Err(format!("Timeout after {:?}", self.timeout));
                let err = ToolError::Timeout {
                    tool_name: tool_name.clone(),
                    timeout: self.timeout,
                };
                (audit, Err(err))
            }
        };

        // Log the execution (both success and failure)
        let audit_entry = AuditEntry {
            timestamp: Utc::now(),
            tool_name: tool_name.clone(),
            args: serde_json::to_value(&args).unwrap_or(serde_json::Value::Null),
            result: audit_result,
            duration_ms: duration.as_millis() as u64,
        };

        // Log audit entry (don't fail the call if audit fails - just warn)
        if let Err(e) = self.audit_logger.log(audit_entry).await {
            eprintln!("Warning: Failed to log audit entry: {}", e);
        }

        return_value
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

#[cfg(test)]
mod tests;
