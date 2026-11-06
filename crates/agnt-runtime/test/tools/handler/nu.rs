use crate::tools::handler::ToolErrorKind;
use crate::tools::handler::builtin_tool::BuiltinTool;
use crate::tools::handler::nu::NuTool;
use agnt_kernel::bus::{CancelEvent, CancelTx};
use std::path::Path;
use std::time::Duration;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
#[ignore]
async fn nu_executes_simple_command() -> Result<()> {
    let cancel = CancelTx::new("test", 8);
    let result = NuTool::execute(
        &serde_json::json!({"command": "echo hello"}),
        Path::new("/tmp"),
        &cancel,
    )
    .await
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result["exit_code"], 0);
    assert!(result["stdout"].as_str().unwrap().contains("hello"));
    Ok(())
}

#[tokio::test]
#[ignore]
async fn nu_captures_stdout_and_stderr() -> Result<()> {
    let cancel = CancelTx::new("test", 8);
    // The command exits non-zero (`error make`), so the captured streams
    // arrive in the failure details payload instead of an Ok payload.
    let err = NuTool::execute(
        &serde_json::json!({"command": "print \"out\"; error make {msg: \"err\"}"}),
        Path::new("/tmp"),
        &cancel,
    )
    .await
    .err()
    .ok_or("non-zero exit must map to Err")?;
    let details = err.details.ok_or("failure must carry output details")?;
    assert!(
        !details["stdout"].as_str().unwrap_or("").is_empty(),
        "stdout must be captured, got: {details}"
    );
    assert!(
        !details["stderr"].as_str().unwrap_or("").is_empty(),
        "stderr must be captured, got: {details}"
    );
    Ok(())
}

#[tokio::test]
async fn nu_invalid_command_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, serde_json::Value)> = vec![
        ("missing command", serde_json::json!({})),
        ("non-string command", serde_json::json!({"command": 42})),
    ];

    // -- Exec & Check
    let cancel = CancelTx::new("test", 8);
    for (name, args) in cases {
        let err = NuTool::execute(&args, Path::new("/tmp"), &cancel)
            .await
            .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Validation, "case '{name}': kind");
    }
}

/// A non-zero nu exit must return a failure-shaped error: kind Runtime,
/// message carrying the exit code, and details carrying stdout/stderr and
/// exit_code — the producer owns the failure state, no text sniffing.
#[tokio::test]
#[ignore]
async fn nu_nonzero_exit_preserved() -> Result<()> {
    let cancel = CancelTx::new("test", 8);
    let err = NuTool::execute(
        &serde_json::json!({"command": "exit 3"}),
        Path::new("/tmp"),
        &cancel,
    )
    .await
    .err()
    .ok_or("non-zero exit must map to Err")?;
    assert_eq!(err.kind, ToolErrorKind::Runtime);
    assert!(
        err.message.contains("exited with code"),
        "message must carry the exit status, got: {}",
        err.message
    );
    let details = err.details.ok_or("error must carry details payload")?;
    assert_eq!(details["exit_code"], 3);
    assert!(
        details["stdout"].is_string() && details["stderr"].is_string(),
        "details must carry stdout and stderr, got: {details}"
    );
    Ok(())
}

#[tokio::test]
#[ignore]
async fn nu_empty_output_handled() -> Result<()> {
    let cancel = CancelTx::new("test", 8);
    let result = NuTool::execute(
        &serde_json::json!({"command": "null"}),
        Path::new("/tmp"),
        &cancel,
    )
    .await
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result["stdout"], "");
    assert_eq!(result["stderr"], "");
    assert_eq!(result["exit_code"], 0);
    Ok(())
}

#[tokio::test]
#[ignore]
async fn nu_handles_large_output_without_deadlock() -> Result<()> {
    let cancel = CancelTx::new("test", 8);
    // Generate ~100KB of output — exceeds typical 64KB pipe buffer
    let result = NuTool::execute(
        &serde_json::json!({"command": "1..10000 | each { |_| 'xxxxxxxxxxxxxxxxxxxx' } | str join (char newline)"}),
        Path::new("/tmp"),
        &cancel,
    )
    .await
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result["exit_code"], 0);
    let stdout = result["stdout"].as_str().unwrap();
    assert!(
        stdout.len() > 64_000,
        "expected >64KB output, got {} bytes",
        stdout.len()
    );
    Ok(())
}

#[tokio::test]
#[ignore]
async fn nu_timeout_kills_process_and_returns_error() -> Result<()> {
    let cancel = CancelTx::new("test", 8);
    let start = std::time::Instant::now();
    let result = NuTool::execute(
        &serde_json::json!({"command": "sleep 2sec", "timeout_seconds": 1}),
        Path::new("/tmp"),
        &cancel,
    )
    .await;
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(5));
    assert!(result.is_err());
    let err = result.err().ok_or("a timeout must return an error")?;
    assert!(err.message.contains("timed out"));
    // The wall clock stopped the call before it produced a result: the error
    // carries the not-run kind, so the live row finishes ⊘ like the replayed.
    assert_eq!(err.kind, ToolErrorKind::NotRun, "timeout kind: {err:?}");
    assert!(
        err.details.is_none(),
        "a timeout carries no output payload: {err:?}"
    );
    Ok(())
}

#[tokio::test]
#[ignore]
async fn nu_cancellation_kills_process_quickly() -> Result<()> {
    let cancel = CancelTx::new("test", 8);
    let cancel2 = cancel.clone();
    let handle = tokio::spawn(async move {
        // Publish once the tool has subscribed to the cancel channel: a
        // broadcast send succeeds only when a receiver is live. Bounded so a
        // missed subscription cannot hang the test.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if cancel2.send(CancelEvent::Requested).await.is_ok() {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::task::yield_now().await;
        }
    });
    let start = std::time::Instant::now();
    let result = NuTool::execute(
        &serde_json::json!({"command": "sleep 2sec"}),
        Path::new("/tmp"),
        &cancel,
    )
    .await;
    handle.await?;
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(5));
    let err = result.err().ok_or("a cancel must return an error")?;
    assert!(err.message.contains("cancelled"));
    // The turn's cancel stopped the call before it produced a result: the same
    // not-run kind the timeout arm carries, so both rehydrate and render ⊘.
    assert_eq!(err.kind, ToolErrorKind::NotRun, "cancel kind: {err:?}");
    Ok(())
}
