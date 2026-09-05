//! Circuit breaker concern for MCP transport failures.
//!
//! Tracks transport failures per MCP server and disables a server's tools
//! once the failure threshold is reached.

use std::error::Error as _;
use std::sync::{Arc, Mutex};

use rig::agent::ToolCallAction;
use rig::tool::{ToolExecutionError, ToolResult};

use crate::bus::{Bus, WarningEvent};
use crate::tools::handler::McpToolRegistry;
use crate::tools::mcp::auth_error::McpAuthError;
use crate::tools::mcp::circuit_breaker::McpCircuitBreaker;

/// Guards MCP tool calls behind a circuit breaker.
#[derive(Clone)]
pub struct CircuitBreakerGuard {
    pub breaker: Arc<Mutex<McpCircuitBreaker>>,
}

impl CircuitBreakerGuard {
    /// Check whether the server that owns `tool_name` has been disabled.
    ///
    /// Returns `Some(ToolCallAction::Skip(...))` if the server is currently disabled, `None` otherwise.
    pub fn check_server_enabled(
        &self,
        tool_name: &str,
        mcp_registry: &McpToolRegistry,
    ) -> Option<ToolCallAction> {
        if let Some(server_name) = mcp_registry.server_name_for(tool_name)
            && !mcp_registry.is_server_enabled(server_name)
        {
            log::trace!(
                "circuit_breaker: MCP server '{server_name}' disabled, skipping {tool_name}"
            );
            return Some(ToolCallAction::skip(format!(
                "MCP server '{server_name}' is disabled (circuit breaker tripped). \
                     Re-enable via MCP panel."
            )));
        }
        None
    }

    /// Record a tool result and update the circuit breaker.
    ///
    /// Classification is structural, over the result's Error disposition:
    ///
    /// - Transport failure — the error's source chain carries an rmcp
    ///   `ServiceError::TransportSend` / `TransportClosed` /
    ///   `UnexpectedResponse` (rig attaches the service error as the
    ///   `ToolExecutionError` source): increments the failure counter and
    ///   potentially trips the breaker, disabling the server.
    /// - Auth error — the error carries HTTP status 401/403: bypasses the
    ///   circuit breaker entirely so user-actionable auth failures don't
    ///   disable the server.
    /// - Anything else — records nothing; a `success` result still records
    ///   success (resetting the failure counter).
    pub async fn record_result(
        &self,
        tool_name: &str,
        raw_result: &ToolResult,
        success: bool,
        mcp_registry: &McpToolRegistry,
        bus: &Bus,
    ) {
        let Some(server_name) = mcp_registry.server_name_for(tool_name) else {
            return;
        };

        // Auth errors bypass the circuit breaker — they are user-actionable
        // (re-login, scope grant) rather than transport-level failures. The
        // HTTP status is the only structural auth signal: rmcp 2.2.0's
        // ServiceError/ErrorData carry no auth-specific structure.
        if let Some(auth_err) = raw_result
            .error()
            .and_then(|e| auth_error_from_status(e, server_name))
        {
            log::error!("{auth_err}");
            return;
        }

        if raw_result.error().is_some_and(source_is_transport) {
            let tripped = {
                let mut cb = self.breaker.lock().expect("circuit breaker mutex poisoned");
                cb.record_failure(server_name)
            };
            if tripped {
                log::warn!(
                    "MCP circuit breaker tripped for server '{}' after transport errors",
                    server_name
                );
                if let Err(e) = mcp_registry.set_server_enabled(server_name, false) {
                    log::error!("Failed to disable MCP server '{}': {}", server_name, e);
                }
                let _ = bus
                    .warning()
                    .send(WarningEvent::Message {
                        message: format!(
                            "MCP server '{server_name}' disconnected — tools disabled. \
                         Re-enable via MCP panel."
                        ),
                    })
                    .await;
            }
        } else if success {
            let mut cb = self.breaker.lock().expect("circuit breaker mutex poisoned");
            cb.record_success(server_name);
        }
    }
}

// region:    --- Support

/// Whether the error's source chain carries an rmcp transport failure.
///
/// rig's rmcp integration attaches the `rmcp::ServiceError` as the
/// `ToolExecutionError` source; a source-chain walk + downcast classifies
/// structurally. `Timeout` and `Cancelled` are deliberately not transport
/// failures: the breaker tracks connection health, not per-call latency.
fn source_is_transport(error: &ToolExecutionError) -> bool {
    let Some(mut source) = error.source() else {
        return false;
    };
    loop {
        if let Some(service_error) = source.downcast_ref::<rmcp::ServiceError>()
            && matches!(
                service_error,
                rmcp::ServiceError::TransportSend(_)
                    | rmcp::ServiceError::TransportClosed
                    | rmcp::ServiceError::UnexpectedResponse
            )
        {
            // `ServiceError` is #[non_exhaustive]: `matches!` desugars with the
            // mandatory wildcard — McpError/Timeout/Cancelled are not transport.
            return true;
        }
        match source.source() {
            Some(next) => source = next,
            None => return false,
        }
    }
}

/// Map the error's HTTP status to the user-actionable auth error to log.
///
/// rmcp 2.2.0's ServiceError/ErrorData carry no auth structure (no HTTP
/// status, no auth error codes — verified), so `http_status()` is the only
/// honest signal. 401 → AuthRequired, 403 → InsufficientScope; auth
/// identifiable only by server prose falls through to the failure branch,
/// which records nothing (same outcome as the bypass).
fn auth_error_from_status(error: &ToolExecutionError, server_name: &str) -> Option<McpAuthError> {
    match error.http_status() {
        Some(401) => Some(McpAuthError::AuthRequired {
            server: server_name.to_string(),
        }),
        Some(403) => Some(McpAuthError::InsufficientScope {
            server: server_name.to_string(),
            required: "see server documentation".to_string(),
        }),
        _ => None,
    }
}

// endregion: --- Support

#[cfg(test)]
#[path = "circuit_breaker_guard_test.rs"]
mod circuit_breaker_guard_test;
