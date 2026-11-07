//! Tests for the Nushell-closure tool adapter.

use super::*;

use crate::tools::audit::AuditLogger;
use crate::tools::closure::{ClosureParameter, EngineInterfaceLike};
use nu_protocol::engine::Closure;
use nu_protocol::{BlockId, Span, Spanned};
use std::sync::Mutex;
use std::time::Duration;

type TestResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ================================================================
// adapt_closures → DynamicTool → ToolServer end-to-end
// ================================================================

// region:    --- Test Support

/// A scripted engine: `eval_closure` records every positional argument list it
/// receives and returns the queued reply.
///
/// `CwdEngine` cannot evaluate closures, so the adapter's real execution path is
/// driven through this stand-in — the same `EngineInterfaceLike` seam the
/// production `nu_plugin::EngineInterface` implements.
struct RecordingEngine {
    reply: Mutex<Option<core::result::Result<Value, String>>>,
    received: Mutex<Vec<Vec<Value>>>,
}

impl RecordingEngine {
    fn replying(reply: core::result::Result<Value, String>) -> Arc<Self> {
        Arc::new(Self {
            reply: Mutex::new(Some(reply)),
            received: Mutex::new(Vec::new()),
        })
    }

    fn received_args(&self) -> Vec<Vec<Value>> {
        self.received
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

impl EngineInterfaceLike for RecordingEngine {
    fn get_span_contents(&self, _span: Span) -> core::result::Result<Vec<u8>, String> {
        Ok(Vec::new())
    }

    fn eval_closure(
        &self,
        _closure: &Spanned<Closure>,
        positional: Vec<Value>,
        _input: Option<Value>,
    ) -> core::result::Result<Value, String> {
        if let Ok(mut received) = self.received.lock() {
            received.push(positional);
        }
        self.reply
            .lock()
            .map(|mut reply| reply.take())
            .unwrap_or(None)
            .unwrap_or_else(|| Ok(Value::nothing(Span::unknown())))
    }
}

fn spanned_closure() -> Spanned<Closure> {
    Spanned {
        item: Closure {
            block_id: BlockId::new(0),
            captures: Vec::new(),
        },
        span: Span::unknown(),
    }
}

fn required_param(name: &str) -> ClosureParameter {
    ClosureParameter {
        name: name.to_string(),
        is_required: true,
    }
}

fn registry_with(name: &str, params: Vec<ClosureParameter>) -> ClosureRegistry {
    let mut registry = ClosureRegistry::default();
    registry.register(
        name.to_string(),
        ResolvedClosure {
            closure: spanned_closure(),
            params,
        },
    );
    registry
}

/// Build a `ToolExecutor` over `engine` with an audit log inside `dir`.
fn executor(engine: Arc<RecordingEngine>, dir: &std::path::Path) -> Arc<ToolExecutor> {
    Arc::new(ToolExecutor::new(
        engine,
        Arc::new(AuditLogger::new(dir.join("audit.log"))),
        Duration::from_secs(5),
    ))
}

/// Register every adapted tool on a real rig `ToolServer` and return the handle.
fn tool_server(tools: Vec<DynamicTool>) -> rig::tool::server::ToolServerHandle {
    let handle = rig::tool::server::ToolServer::new().run();
    for tool in tools {
        handle.add_dynamic_tool(tool);
    }
    handle
}

// endregion: --- Test Support

#[tokio::test]
async fn adapt_closures_returns_one_dynamic_tool_per_registered_closure() -> TestResult<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let engine = RecordingEngine::replying(Ok(Value::nothing(Span::unknown())));
    let mut registry = registry_with("echo", vec![required_param("x")]);
    registry.register(
        "noop".to_string(),
        ResolvedClosure {
            closure: spanned_closure(),
            params: Vec::new(),
        },
    );

    // -- Exec
    let tools = adapt_closures(&registry, executor(engine, temp.path()), Span::unknown(), 0)
        .map_err(|e| format!("adapt_closures must succeed: {e}"))?;

    // -- Check
    let mut names: Vec<&str> = tools.iter().map(|tool| tool.name().as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["echo", "noop"]);

    let echo = tools
        .iter()
        .find(|tool| tool.name().as_str() == "echo")
        .ok_or("the echo closure must produce a tool")?;
    assert_eq!(echo.definition().description, "Nushell closure tool: echo");
    assert_eq!(
        echo.definition().parameters["properties"]["x"]["type"],
        "string"
    );
    assert_eq!(echo.definition().parameters["required"][0], "x");

    let noop = tools
        .iter()
        .find(|tool| tool.name().as_str() == "noop")
        .ok_or("the noop closure must produce a tool")?;
    assert_eq!(
        noop.definition().parameters["properties"]
            .as_object()
            .map(|props| props.len()),
        Some(0),
        "a closure without parameters must expose an empty schema"
    );
    Ok(())
}

#[tokio::test]
async fn adapt_closures_tool_invocation_converts_json_args_and_result() -> TestResult<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let engine = RecordingEngine::replying(Ok(Value::string("closure-ran", Span::unknown())));
    let registry = registry_with("echo", vec![required_param("x")]);
    let tools = adapt_closures(
        &registry,
        executor(Arc::clone(&engine), temp.path()),
        Span::unknown(),
        0,
    )
    .map_err(|e| format!("adapt_closures must succeed: {e}"))?;
    let handle = tool_server(tools);

    // -- Exec
    let defs = handle.snapshot();
    let mut context = rig::tool::ToolContext::new();
    let result = handle
        .toolset()
        .execute("echo", r#"{"x":"hello"}"#, &mut context)
        .await;

    // -- Check
    assert!(
        defs.definitions()
            .iter()
            .any(|def| def.name.as_str() == "echo"),
        "the adapted closure must be registered on the tool server"
    );
    assert!(
        result.is_success(),
        "the closure tool call must succeed: {result:?}"
    );
    assert_eq!(
        result.output().render(),
        "\"closure-ran\"",
        "the closure result must be returned as JSON"
    );

    let received = engine.received_args();
    assert_eq!(
        received.len(),
        1,
        "the closure must be invoked exactly once"
    );
    let args = received.first().ok_or("one invocation")?;
    assert_eq!(
        args.len(),
        1,
        "the closure must receive one positional argument"
    );
    assert_eq!(
        args[0].as_str().ok(),
        Some("hello"),
        "the JSON argument must convert to a Nushell string"
    );
    Ok(())
}

#[tokio::test]
async fn adapt_closures_tool_invocation_serializes_structured_result() -> TestResult<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let span = Span::unknown();
    let engine = RecordingEngine::replying(Ok(Value::record(
        nu_protocol::record! {
            "count" => Value::int(3, span),
            "ok" => Value::bool(true, span),
            "tags" => Value::list(
                vec![Value::string("a", span), Value::string("b", span)],
                span,
            ),
        },
        span,
    )));
    let registry = registry_with("stats", vec![required_param("x")]);
    let tools = adapt_closures(&registry, executor(engine, temp.path()), span, 0)
        .map_err(|e| format!("adapt_closures must succeed: {e}"))?;
    let handle = tool_server(tools);

    // -- Exec
    let mut context = rig::tool::ToolContext::new();
    let result = handle
        .toolset()
        .execute("stats", r#"{"x":"ignored"}"#, &mut context)
        .await;

    // -- Check
    assert!(
        result.is_success(),
        "the closure tool call must succeed: {result:?}"
    );
    let parsed: serde_json::Value = serde_json::from_str(&result.output().render())?;
    assert_eq!(
        parsed["count"], 3,
        "an int must round-trip as a JSON number"
    );
    assert_eq!(parsed["ok"], true, "a bool must round-trip as a JSON bool");
    assert_eq!(
        parsed["tags"],
        serde_json::json!(["a", "b"]),
        "a list must round-trip as a JSON array"
    );
    Ok(())
}

#[tokio::test]
async fn adapt_closures_tool_missing_argument_passes_nothing() -> TestResult<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let engine = RecordingEngine::replying(Ok(Value::nothing(Span::unknown())));
    let registry = registry_with("echo", vec![required_param("x")]);
    let tools = adapt_closures(
        &registry,
        executor(Arc::clone(&engine), temp.path()),
        Span::unknown(),
        0,
    )
    .map_err(|e| format!("adapt_closures must succeed: {e}"))?;
    let handle = tool_server(tools);

    // -- Exec
    let mut context = rig::tool::ToolContext::new();
    let result = handle.toolset().execute("echo", "{}", &mut context).await;

    // -- Check
    assert!(
        result.is_success(),
        "a missing argument must not fail the call: {result:?}"
    );
    let received = engine.received_args();
    let args = received.first().ok_or("one invocation")?;
    assert!(
        args[0].is_nothing(),
        "a missing argument must convert to Nushell nothing"
    );
    Ok(())
}

#[tokio::test]
async fn adapt_closures_tool_truncates_output_at_max_bytes() -> TestResult<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let span = Span::unknown();
    let engine = RecordingEngine::replying(Ok(Value::string("y".repeat(500), span)));
    let registry = registry_with("big", vec![required_param("x")]);
    let tools = adapt_closures(&registry, executor(engine, temp.path()), span, 64)
        .map_err(|e| format!("adapt_closures must succeed: {e}"))?;
    let handle = tool_server(tools);

    // -- Exec
    let mut context = rig::tool::ToolContext::new();
    let result = handle
        .toolset()
        .execute("big", r#"{"x":"a"}"#, &mut context)
        .await;

    // -- Check
    assert!(
        result.is_success(),
        "the closure tool call must succeed: {result:?}"
    );
    let output = result.output().render();
    assert!(
        output.contains("[output truncated:"),
        "output above max_tool_result_bytes must be truncated, got: {output}"
    );
    assert!(
        output.len() < 500,
        "the truncated output must be shorter than the raw result, got {} bytes",
        output.len()
    );
    Ok(())
}

#[tokio::test]
async fn adapt_closures_tool_maps_engine_failure_to_tool_error() -> TestResult<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let engine = RecordingEngine::replying(Err("engine exploded".to_string()));
    let registry = registry_with("boom", vec![required_param("x")]);
    let tools = adapt_closures(&registry, executor(engine, temp.path()), Span::unknown(), 0)
        .map_err(|e| format!("adapt_closures must succeed: {e}"))?;
    let handle = tool_server(tools);

    // -- Exec
    let mut context = rig::tool::ToolContext::new();
    let result = handle
        .toolset()
        .execute("boom", r#"{"x":"a"}"#, &mut context)
        .await;

    // -- Check
    assert!(
        !result.is_success(),
        "an engine failure must fail the tool call"
    );
    let error = result.error().ok_or("the failure must carry an error")?;
    assert_eq!(error.kind(), rig::tool::ToolErrorKind::Other);
    assert!(
        error.message().contains("Closure execution failed"),
        "the engine failure must stay model-visible, got: {}",
        error.message()
    );
    Ok(())
}

#[tokio::test]
async fn adapt_closures_tool_rejects_unconvertible_result() -> TestResult<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let span = Span::unknown();
    let engine =
        RecordingEngine::replying(Ok(Value::date(chrono::Utc::now().fixed_offset(), span)));
    let registry = registry_with("dated", vec![required_param("x")]);
    let tools = adapt_closures(&registry, executor(engine, temp.path()), span, 0)
        .map_err(|e| format!("adapt_closures must succeed: {e}"))?;
    let handle = tool_server(tools);

    // -- Exec
    let mut context = rig::tool::ToolContext::new();
    let result = handle
        .toolset()
        .execute("dated", r#"{"x":"a"}"#, &mut context)
        .await;

    // -- Check
    assert!(
        !result.is_success(),
        "a result with no JSON equivalent must fail the tool call"
    );
    let error = result.error().ok_or("the failure must carry an error")?;
    assert!(
        error.message().contains("Nu to JSON conversion failed"),
        "the conversion failure must be named, got: {}",
        error.message()
    );
    Ok(())
}

#[tokio::test]
async fn adapt_closures_empty_registry_returns_no_tools() -> TestResult<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let engine = RecordingEngine::replying(Ok(Value::nothing(Span::unknown())));

    // -- Exec
    let tools = adapt_closures(
        &ClosureRegistry::default(),
        executor(engine, temp.path()),
        Span::unknown(),
        0,
    )
    .map_err(|e| format!("adapt_closures must succeed: {e}"))?;

    // -- Check
    assert!(tools.is_empty(), "an empty registry must produce no tools");
    Ok(())
}

#[tokio::test]
async fn adapt_closures_empty_closure_name_is_rejected() -> TestResult<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let engine = RecordingEngine::replying(Ok(Value::nothing(Span::unknown())));
    let registry = registry_with("", vec![required_param("x")]);

    // -- Exec
    let result = adapt_closures(&registry, executor(engine, temp.path()), Span::unknown(), 0);

    // -- Check
    assert!(
        result.is_err(),
        "an empty closure name must be rejected before registration"
    );
    Ok(())
}
