//! Circuit breaker concern for MCP transport failures.
//!
//! Tracks transport failures per MCP server and disables a server's tools
//! once the failure threshold is reached.

use std::sync::{Arc, Mutex};

use rig::agent::ToolCallHookAction;
use tokio::sync::mpsc;

use crate::protocol::event::UiEvent;
use crate::tools::handler::McpToolRegistry;
use crate::tools::mcp::circuit_breaker::{McpCircuitBreaker, is_transport_error};

/// Guards MCP tool calls behind a circuit breaker.
#[derive(Clone)]
pub struct CircuitBreakerGuard {
    pub breaker: Arc<Mutex<McpCircuitBreaker>>,
}

impl CircuitBreakerGuard {
    /// Check whether the server that owns `tool_name` has been disabled.
    ///
    /// Returns `Some(Skip)` if the server is currently disabled, `None` otherwise.
    pub fn check_server_enabled(
        &self,
        tool_name: &str,
        mcp_registry: &McpToolRegistry,
    ) -> Option<ToolCallHookAction> {
        if let Some(server_name) = mcp_registry.server_name_for(tool_name)
            && !mcp_registry.is_server_enabled(server_name)
        {
            log::trace!(
                "circuit_breaker: MCP server '{server_name}' disabled, skipping {tool_name}"
            );
            return Some(ToolCallHookAction::Skip {
                reason: format!(
                    "MCP server '{}' is disabled (circuit breaker tripped). \
                     Re-enable via MCP panel.",
                    server_name
                ),
            });
        }
        None
    }

    /// Record a tool result and update the circuit breaker.
    ///
    /// If the result is a transport error, increments the failure counter and
    /// potentially trips the breaker, disabling the server.
    pub fn record_result(
        &self,
        tool_name: &str,
        result: &str,
        success: bool,
        mcp_registry: &McpToolRegistry,
        ui_tx: &mpsc::UnboundedSender<UiEvent>,
    ) {
        let Some(server_name) = mcp_registry.server_name_for(tool_name) else {
            return;
        };

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
                let _ = ui_tx.send(UiEvent::Warning {
                    message: format!(
                        "MCP server '{}' disconnected — tools disabled. \
                         Re-enable via MCP panel.",
                        server_name
                    ),
                });
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
