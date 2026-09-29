use crate::bus::{Bus, CancelEvent};
use crate::protocol::tool_args::CallLine;
use crate::tools::handler::ToolErrorKind;
use crate::tools::handler::builtin_tool::{BuiltinTool, Previewable};
use crate::tools::handler::nu::NuTool;
use crate::transcript::ir::ContentKind;
use std::path::Path;
use std::time::Duration;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
#[ignore]
async fn nu_executes_simple_command() -> Result<()> {
    let bus = Bus::default();
    let result = NuTool::execute(
        &serde_json::json!({"command": "echo hello"}),
        Path::new("/tmp"),
        &bus,
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
    let bus = Bus::default();
    // The command exits non-zero (`error make`), so the captured streams
    // arrive in the failure details payload instead of an Ok payload.
    let err = NuTool::execute(
        &serde_json::json!({"command": "print \"out\"; error make {msg: \"err\"}"}),
        Path::new("/tmp"),
        &bus,
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
async fn nu_missing_command_returns_validation_error() {
    let bus = Bus::default();
    let err = NuTool::execute(&serde_json::json!({}), Path::new("/tmp"), &bus)
        .await
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
}

#[tokio::test]
async fn nu_non_string_command_returns_validation_error() {
    let bus = Bus::default();
    let err = NuTool::execute(&serde_json::json!({"command": 42}), Path::new("/tmp"), &bus)
        .await
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
}

/// A non-zero nu exit must return a failure-shaped error: kind Runtime,
/// message carrying the exit code, and details carrying stdout/stderr and
/// exit_code — the producer owns the failure state, no text sniffing.
#[tokio::test]
#[ignore]
async fn nu_nonzero_exit_preserved() -> Result<()> {
    let bus = Bus::default();
    let err = NuTool::execute(
        &serde_json::json!({"command": "exit 3"}),
        Path::new("/tmp"),
        &bus,
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
    let bus = Bus::default();
    let result = NuTool::execute(
        &serde_json::json!({"command": "null"}),
        Path::new("/tmp"),
        &bus,
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
    let bus = Bus::default();
    // Generate ~100KB of output — exceeds typical 64KB pipe buffer
    let result = NuTool::execute(
        &serde_json::json!({"command": "1..10000 | each { |_| 'xxxxxxxxxxxxxxxxxxxx' } | str join (char newline)"}),
        Path::new("/tmp"),
        &bus,
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
async fn nu_timeout_kills_process_and_returns_error() {
    let bus = Bus::default();
    let start = std::time::Instant::now();
    let result = NuTool::execute(
        &serde_json::json!({"command": "sleep 10sec", "timeout_seconds": 1}),
        Path::new("/tmp"),
        &bus,
    )
    .await;
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(5));
    assert!(result.is_err());
    assert!(result.unwrap_err().message.contains("timed out"));
}

#[tokio::test]
#[ignore]
async fn nu_cancellation_kills_process_quickly() {
    let bus = Bus::default();
    let bus2 = bus.clone();
    let handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = bus2.cancel().send(CancelEvent::Requested).await;
    });
    let start = std::time::Instant::now();
    let result = NuTool::execute(
        &serde_json::json!({"command": "sleep 30sec"}),
        Path::new("/tmp"),
        &bus,
    )
    .await;
    handle.await.unwrap();
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(5));
    assert!(result.is_err());
    assert!(result.unwrap_err().message.contains("cancelled"));
}

// === Previewable ===

#[test]
fn nu_preview_returns_code_display() -> Result<()> {
    // -- Setup & Fixtures
    let args = serde_json::json!({"command": "ls | select name"});

    // -- Exec
    let display = NuTool::preview(&args, Path::new("/tmp"));

    // -- Check
    let display = display.ok_or("preview must produce a display for a valid command")?;
    assert_eq!(display.title, "nu");
    assert_eq!(display.sections.len(), 1);
    let section = &display.sections[0];
    assert!(
        matches!(section.kind, ContentKind::Code { ref language } if language == "nu"),
        "section kind must be Code(nu), got {:?}",
        section.kind
    );
    assert_eq!(section.content, "ls | select name");
    assert!(section.stats.is_none(), "code preview carries no stats");
    Ok(())
}

#[test]
fn nu_preview_empty_command_returns_none() -> Result<()> {
    // -- Setup & Fixtures
    let args = serde_json::json!({"command": ""});

    // -- Exec & Check
    assert!(
        NuTool::preview(&args, Path::new("/tmp")).is_none(),
        "empty command must produce no preview"
    );
    Ok(())
}

#[test]
fn nu_preview_missing_command_returns_none() -> Result<()> {
    // -- Setup & Fixtures
    let args = serde_json::json!({});

    // -- Exec & Check
    assert!(
        NuTool::preview(&args, Path::new("/tmp")).is_none(),
        "missing command must produce no preview"
    );
    Ok(())
}

// === call_line_render ===

#[test]
fn nu_call_line_render_returns_command_summary() -> Result<()> {
    // -- Setup & Fixtures
    let args = r#"{"command":"ls"}"#;

    // -- Exec
    let render = NuTool::call_line_render(args);

    // -- Check: the command renders in the preview block, so the call line
    // carries no summary and row 0 shows the tool name alone.
    assert_eq!(
        render,
        CallLine {
            summary: String::new()
        }
    );
    Ok(())
}

#[test]
fn nu_call_line_render_falls_back_to_generic_on_invalid_json() -> Result<()> {
    // -- Setup & Fixtures
    let args = "not-json";

    // -- Exec
    let render = NuTool::call_line_render(args);

    // -- Check
    assert_eq!(render, CallLine::from_json_summary(args));
    Ok(())
}

#[test]
fn nu_call_line_render_falls_back_to_generic_on_missing_or_empty_command() -> Result<()> {
    // -- Setup & Fixtures
    let missing = r#"{"timeout_seconds":5}"#;
    let empty = r#"{"command":""}"#;

    // -- Exec & Check
    for args in [missing, empty] {
        assert_eq!(
            NuTool::call_line_render(args),
            CallLine::from_json_summary(args),
            "args without a usable command must keep the generic summary: {args}"
        );
    }
    Ok(())
}
