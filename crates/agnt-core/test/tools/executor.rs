use std::time::Duration;

// ---------------------------------------------------------------------------
// Behavioral tests: closure invocation, audit logging, result handling
// ---------------------------------------------------------------------------

use super::*;

use std::path::PathBuf;
use std::sync::Mutex;

use nu_protocol::BlockId;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// A scripted engine: `eval_closure` sleeps for `delay`, then returns the
/// queued reply (or `nothing` when the queue is empty).
struct MockEngine {
    reply: Mutex<Option<core::result::Result<Value, String>>>,
    delay: Duration,
}

impl MockEngine {
    fn replying(reply: core::result::Result<Value, String>) -> Self {
        Self {
            reply: Mutex::new(Some(reply)),
            delay: Duration::ZERO,
        }
    }

    fn delaying(delay: Duration) -> Self {
        Self {
            reply: Mutex::new(None),
            delay,
        }
    }
}

impl EngineInterfaceLike for MockEngine {
    fn get_span_contents(&self, _span: Span) -> core::result::Result<Vec<u8>, String> {
        Ok(Vec::new())
    }

    fn eval_closure(
        &self,
        _closure: &Spanned<Closure>,
        _positional: Vec<Value>,
        _input: Option<Value>,
    ) -> core::result::Result<Value, String> {
        if !self.delay.is_zero() {
            std::thread::sleep(self.delay);
        }
        self.reply
            .lock()
            .unwrap()
            .take()
            .unwrap_or(Ok(Value::nothing(Span::unknown())))
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

fn executor(engine: MockEngine, log_path: PathBuf, timeout: Duration) -> ToolExecutor {
    ToolExecutor::new(
        Arc::new(engine),
        Arc::new(AuditLogger::new(log_path)),
        timeout,
    )
}

fn read_log(path: &std::path::Path) -> Result<String> {
    Ok(std::fs::read_to_string(path)?)
}

// endregion: --- Test Support

#[tokio::test]
async fn test_tools_executor_invoke_closure_returns_value_and_logs_ok() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let log_path = temp.path().join("audit.log");
    let executor = executor(
        MockEngine::replying(Ok(Value::int(7, Span::unknown()))),
        log_path.clone(),
        Duration::from_secs(5),
    );

    // -- Exec
    let result = executor
        .invoke_closure(
            &spanned_closure(),
            vec![Value::int(1, Span::unknown())],
            Span::unknown(),
        )
        .await;

    // -- Check
    let value = result.map_err(|e| format!("invoke_closure must succeed: {e:?}"))?;
    assert_eq!(value.as_int().map_err(|e| format!("{e:?}"))?, 7);

    let log = read_log(&log_path)?;
    let entry: serde_json::Value = serde_json::from_str(log.trim())?;
    assert!(
        entry["tool_name"]
            .as_str()
            .unwrap_or("")
            .contains("Closure"),
        "audit log must record the tool name, got: {log}"
    );
    assert!(
        entry["duration_ms"].is_number(),
        "audit log must record duration_ms, got: {log}"
    );
    assert_eq!(
        entry["result"]["Int"]["val"], 7,
        "audit log must record the Ok result, got: {log}"
    );
    Ok(())
}

#[tokio::test]
async fn test_tools_executor_invoke_closure_propagates_execution_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let log_path = temp.path().join("audit.log");
    let executor = executor(
        MockEngine::replying(Err("boom".to_string())),
        log_path.clone(),
        Duration::from_secs(5),
    );

    // -- Exec
    let result = executor
        .invoke_closure(&spanned_closure(), Vec::new(), Span::unknown())
        .await;

    // -- Check
    let err = result.err().ok_or("an engine failure must fail the call")?;
    assert!(
        matches!(err, ToolError::Execution(_)),
        "an engine failure must surface as ToolError::Execution, got: {err:?}"
    );
    let log = read_log(&log_path)?;
    assert!(
        log.contains("Execution error: Closure execution failed"),
        "audit log must record the execution error, got: {log}"
    );
    Ok(())
}

#[tokio::test]
async fn test_tools_executor_invoke_closure_times_out() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let log_path = temp.path().join("audit.log");
    let executor = executor(
        MockEngine::delaying(Duration::from_millis(300)),
        log_path.clone(),
        Duration::from_millis(20),
    );

    // -- Exec
    let result = executor
        .invoke_closure(&spanned_closure(), Vec::new(), Span::unknown())
        .await;

    // -- Check
    let err = result.err().ok_or("a slow closure must time out")?;
    assert!(
        matches!(err, ToolError::Timeout { .. }),
        "a slow closure must surface as ToolError::Timeout, got: {err:?}"
    );
    let log = read_log(&log_path)?;
    assert!(
        log.contains("Timeout after"),
        "audit log must record the timeout, got: {log}"
    );
    Ok(())
}

#[tokio::test]
async fn test_tools_executor_invoke_closure_survives_audit_log_failure() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    // A log path whose parent directory does not exist: `log()` fails and the
    // executor must warn rather than fail the call.
    let log_path = temp.path().join("missing-dir").join("audit.log");
    let executor = executor(
        MockEngine::replying(Ok(Value::int(3, Span::unknown()))),
        log_path,
        Duration::from_secs(5),
    );

    // -- Exec
    let result = executor
        .invoke_closure(&spanned_closure(), Vec::new(), Span::unknown())
        .await;

    // -- Check
    let value = result.map_err(|e| format!("an audit failure must not fail the call: {e:?}"))?;
    assert_eq!(value.as_int().map_err(|e| format!("{e:?}"))?, 3);
    Ok(())
}

#[tokio::test]
async fn test_tools_executor_accessors_expose_engine_and_timeout() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let executor = executor(
        MockEngine::replying(Ok(Value::nothing(Span::unknown()))),
        temp.path().join("audit.log"),
        Duration::from_secs(42),
    );

    // -- Exec & Check
    assert_eq!(executor.timeout(), Duration::from_secs(42));
    assert!(
        executor.engine().get_span_contents(Span::unknown()).is_ok(),
        "the engine accessor must expose the injected engine"
    );
    Ok(())
}
