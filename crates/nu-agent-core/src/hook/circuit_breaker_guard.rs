//! Circuit breaker concern for MCP transport failures.
//!
//! Tracks transport failures per MCP server and disables a server's tools
//! once the failure threshold is reached.

use std::sync::{Arc, Mutex};

use rig::agent::ToolCallAction;

use crate::bus::{Bus, WarningEvent};
use crate::tools::handler::McpToolRegistry;
use crate::tools::mcp::circuit_breaker::{McpCircuitBreaker, is_transport_error};
use crate::tools::mcp::runtime::classify_mcp_error;

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
    /// If the result is a transport error, increments the failure counter and
    /// potentially trips the breaker, disabling the server.
    ///
    /// Auth errors (401/403) are NOT transport errors — they bypass the circuit
    /// breaker entirely so that user-actionable auth failures don't disable the
    /// server.
    pub async fn record_result(
        &self,
        tool_name: &str,
        result: &str,
        success: bool,
        mcp_registry: &McpToolRegistry,
        bus: &Bus,
    ) {
        let Some(server_name) = mcp_registry.server_name_for(tool_name) else {
            return;
        };

        // Auth errors bypass the circuit breaker — they are user-actionable
        // (re-login, scope grant) rather than transport-level failures.
        if let Some(auth_err) = classify_mcp_error(result, server_name) {
            log::error!("{auth_err}");
            return;
        }

        if is_transport_error(result) {
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

#[cfg(test)]
#[path = "circuit_breaker_guard_test.rs"]
mod circuit_breaker_guard_test;
