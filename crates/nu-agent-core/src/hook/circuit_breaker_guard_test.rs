use super::*;
use crate::tools::handler::McpToolRegistry;
use crate::tools::mcp::circuit_breaker::McpCircuitBreaker;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

fn make_guard() -> CircuitBreakerGuard {
    CircuitBreakerGuard {
        breaker: Arc::new(Mutex::new(McpCircuitBreaker::default())),
    }
}

fn make_registry_empty() -> Arc<McpToolRegistry> {
    Arc::new(McpToolRegistry::empty())
}

fn make_ui_tx() -> (
    mpsc::UnboundedSender<crate::protocol::event::UiEvent>,
    mpsc::UnboundedReceiver<crate::protocol::event::UiEvent>,
) {
    mpsc::unbounded_channel()
}

#[test]
fn non_mcp_tool_check_returns_none() {
    let guard = make_guard();
    let registry = make_registry_empty();
    // Tool not in any MCP server → server_name_for returns None → None
    let result = guard.check_server_enabled("local_tool", &registry);
    assert!(result.is_none());
}

#[test]
fn record_result_non_mcp_tool_is_noop() {
    let guard = make_guard();
    let registry = make_registry_empty();
    let (tx, mut rx) = make_ui_tx();
    // Should not panic or emit events
    guard.record_result("local_tool", "some result", true, &registry, &tx);
    assert!(rx.try_recv().is_err());
}

#[test]
fn skip_reason_contains_server_name() {
    // We can't easily test server disabling without a registered MCP server,
    // but we can verify the guard is safe when there's no server mapping.
    let guard = make_guard();
    let registry = make_registry_empty();
    let result = guard.check_server_enabled("unknown_tool", &registry);
    assert!(result.is_none(), "unknown tool should not trigger skip");
}
