use std::any::TypeId;
use std::sync::{Arc, Mutex};

use rig::tool::{ToolExecutionError, ToolResult};

use super::*;
use crate::bus::create_bus;
use crate::tools::handler::McpToolRegistry;
use crate::tools::mcp::circuit_breaker::McpCircuitBreaker;
use crate::tools::mcp::client::McpToolDefinition;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn make_guard(threshold: usize) -> CircuitBreakerGuard {
    CircuitBreakerGuard {
        breaker: Arc::new(Mutex::new(McpCircuitBreaker::new(threshold))),
    }
}

fn make_registry_empty() -> Arc<McpToolRegistry> {
    Arc::new(McpToolRegistry::empty())
}

fn make_registry(tool_name: &str, server_name: &str) -> Result<Arc<McpToolRegistry>> {
    // -- Setup & Fixtures
    let registry = McpToolRegistry::from_tools([McpToolDefinition {
        server: server_name.to_string(),
        name: tool_name.to_string(),
        raw_name: tool_name.to_string(),
        description: None,
        parameters: None,
    }])?;
    Ok(Arc::new(registry))
}

#[test]
fn non_mcp_tool_check_returns_none() {
    let guard = make_guard(1);
    let registry = make_registry_empty();
    // Tool not in any MCP server → server_name_for returns None → None
    let result = guard.check_server_enabled("local_tool", &registry);
    assert!(result.is_none());
}

#[tokio::test]
async fn record_result_non_mcp_tool_is_noop() {
    let guard = make_guard(1);
    let registry = make_registry_empty();
    let bus = create_bus();
    // Even a transport-classified error must be a no-op for a tool that maps
    // to no MCP server: no panic, no state change, no emitted events.
    guard
        .record_result(
            "local_tool",
            &transport_closed_result(),
            true,
            &registry,
            &bus,
        )
        .await;
}

#[test]
fn skip_reason_contains_server_name() {
    // We can't easily test server disabling without a registered MCP server,
    // but we can verify the guard is safe when there's no server mapping.
    let guard = make_guard(1);
    let registry = make_registry_empty();
    let result = guard.check_server_enabled("unknown_tool", &registry);
    assert!(result.is_none(), "unknown tool should not trigger skip");
}

#[tokio::test]
async fn transport_source_variant_records_failure_and_trips_breaker() -> Result<()> {
    // -- Exec & Check
    // Every rmcp variant the classifier treats as a transport failure must
    // record a breaker failure: at threshold 1 a single call trips the
    // breaker and disables the server — the observable of record_failure.
    for (label, raw_result) in [
        ("TransportClosed", transport_closed_result()),
        ("TransportSend", transport_send_result()),
        ("UnexpectedResponse", unexpected_response_result()),
    ] {
        let guard = make_guard(1);
        let registry = make_registry("mcp__srv__ping", "srv")?;
        let bus = create_bus();

        guard
            .record_result("mcp__srv__ping", &raw_result, false, &registry, &bus)
            .await;

        assert!(
            !registry.is_server_enabled("srv"),
            "{label}: transport-classified failure must trip the breaker and disable the server"
        );
        assert!(
            guard
                .check_server_enabled("mcp__srv__ping", &registry)
                .is_some(),
            "{label}: tripped breaker must make check_server_enabled return a skip action"
        );
    }
    Ok(())
}

#[tokio::test]
async fn transport_source_found_through_error_source_chain() -> Result<()> {
    // -- Setup & Fixtures
    // The rmcp ServiceError is not the immediate source: it sits deeper in the
    // error chain. The classifier must walk `source()`, not just downcast the
    // first hop.
    let guard = make_guard(1);
    let registry = make_registry("mcp__srv__ping", "srv")?;
    let bus = create_bus();

    let raw_result = ToolResult::failed(
        ToolExecutionError::provider("outer failure")
            .with_source(WrappedServiceError(rmcp::ServiceError::TransportClosed)),
    );

    // -- Exec
    guard
        .record_result("mcp__srv__ping", &raw_result, false, &registry, &bus)
        .await;

    // -- Check
    assert!(
        !registry.is_server_enabled("srv"),
        "chain walk must find the rmcp ServiceError and record the transport failure"
    );
    Ok(())
}

#[tokio::test]
async fn auth_http_status_bypasses_breaker_without_recording() -> Result<()> {
    // -- Exec & Check
    // Auth errors are user-actionable, not transport failures: they must never
    // reach record_failure — even at threshold 1 and across repeated calls.
    for status in [401u16, 403] {
        let guard = make_guard(1);
        let registry = make_registry("mcp__srv__ping", "srv")?;
        let bus = create_bus();
        let raw_result = ToolResult::failed(
            ToolExecutionError::provider("auth failed").with_http_status(status),
        );

        for _ in 0..3 {
            guard
                .record_result("mcp__srv__ping", &raw_result, false, &registry, &bus)
                .await;
        }

        assert!(
            registry.is_server_enabled("srv"),
            "HTTP {status}: auth errors must bypass the breaker — no failure recorded"
        );
        assert!(
            guard
                .check_server_enabled("mcp__srv__ping", &registry)
                .is_none(),
            "HTTP {status}: bypassed breaker must not skip the tool"
        );
    }
    Ok(())
}

#[tokio::test]
async fn unclassified_error_records_nothing() -> Result<()> {
    // -- Setup & Fixtures
    // A sourceless error has no transport source and no auth signal: it must
    // record nothing (same outcome as the auth bypass). A rmcp Timeout is
    // deliberately NOT a transport failure — the breaker tracks connection
    // health, not per-call latency — so two of them at threshold 1 must not
    // trip either.
    let guard = make_guard(1);
    let registry = make_registry("mcp__srv__ping", "srv")?;
    let bus = create_bus();

    let timeout_result = ToolResult::failed(
        ToolExecutionError::provider("MCP tool 'x' timed out").with_source(
            rmcp::ServiceError::Timeout {
                timeout: std::time::Duration::from_secs(1),
            },
        ),
    );

    // -- Exec
    guard
        .record_result(
            "mcp__srv__ping",
            &ToolResult::failed(ToolExecutionError::provider("boom")),
            false,
            &registry,
            &bus,
        )
        .await;
    guard
        .record_result("mcp__srv__ping", &timeout_result, false, &registry, &bus)
        .await;

    // -- Check
    assert!(
        registry.is_server_enabled("srv"),
        "unclassified errors must record nothing — the breaker must not trip"
    );
    assert!(
        guard
            .check_server_enabled("mcp__srv__ping", &registry)
            .is_none(),
        "unclassified errors must not skip the tool"
    );
    Ok(())
}

#[tokio::test]
async fn success_resets_failure_count() -> Result<()> {
    // -- Setup & Fixtures
    let guard = make_guard(2);
    let registry = make_registry("mcp__srv__ping", "srv")?;
    let bus = create_bus();

    // -- Exec
    // One transport failure (count 1 of 2, no trip)...
    guard
        .record_result(
            "mcp__srv__ping",
            &transport_closed_result(),
            false,
            &registry,
            &bus,
        )
        .await;
    // ...a success must reset the counter...
    let ok = ToolResult::success(rig::tool::ToolOutput::text("ok"));
    guard
        .record_result("mcp__srv__ping", &ok, true, &registry, &bus)
        .await;
    // ...so the next transport failure starts from 1 again and must not trip.
    guard
        .record_result(
            "mcp__srv__ping",
            &transport_closed_result(),
            false,
            &registry,
            &bus,
        )
        .await;

    // -- Check
    assert!(
        registry.is_server_enabled("srv"),
        "success must record_success (reset the counter) — a single later failure must not trip"
    );
    Ok(())
}

// -- Test Support

fn transport_closed_result() -> ToolResult {
    ToolResult::failed(
        ToolExecutionError::provider("MCP tool 'x' request failed: Transport closed")
            .with_source(rmcp::ServiceError::TransportClosed),
    )
}

fn transport_send_result() -> ToolResult {
    ToolResult::failed(
        ToolExecutionError::provider(
            "MCP tool 'x' request failed: Transport send error: broken pipe",
        )
        .with_source(rmcp::ServiceError::TransportSend(
            rmcp::transport::DynamicTransportError::from_parts(
                "streamable-http-client",
                TypeId::of::<()>(),
                Box::new(std::io::Error::other("broken pipe")),
            ),
        )),
    )
}

fn unexpected_response_result() -> ToolResult {
    ToolResult::failed(
        ToolExecutionError::provider("MCP tool 'x' request failed: Unexpected response type")
            .with_source(rmcp::ServiceError::UnexpectedResponse),
    )
}

/// A wrapper error whose `source()` is the rmcp ServiceError — mirrors an
/// error chain where the service error is not the immediate source.
#[derive(Debug)]
struct WrappedServiceError(rmcp::ServiceError);

impl std::fmt::Display for WrappedServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "wrapped: {}", self.0)
    }
}

impl std::error::Error for WrappedServiceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}
