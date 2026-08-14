use crate::bus::{Bus, CancelEvent};
use crate::tools::handler::ToolErrorKind;
use crate::tools::handler::builtin_tool::BuiltinTool;
use crate::tools::handler::nu::NuTool;
use std::path::Path;
use std::time::Duration;

#[tokio::test]
async fn nu_executes_simple_command() {
    let bus = Bus::new();
    let result = NuTool::execute(
        &serde_json::json!({"command": "echo hello"}),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap();
    assert_eq!(result["exit_code"], 0);
    assert!(result["stdout"].as_str().unwrap().contains("hello"));
}

#[tokio::test]
async fn nu_captures_stdout_and_stderr() {
    let bus = Bus::new();
    let result = NuTool::execute(
        &serde_json::json!({"command": "print \"out\"; error make {msg: \"err\"}"}),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap();
    assert!(!result["stdout"].as_str().unwrap().is_empty());
    assert!(!result["stderr"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn nu_missing_command_returns_validation_error() {
    let bus = Bus::new();
    let err = NuTool::execute(&serde_json::json!({}), Path::new("/tmp"), &bus)
        .await
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
}

#[tokio::test]
async fn nu_non_string_command_returns_validation_error() {
    let bus = Bus::new();
    let err = NuTool::execute(&serde_json::json!({"command": 42}), Path::new("/tmp"), &bus)
        .await
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
}

#[tokio::test]
async fn nu_nonzero_exit_preserved() {
    let bus = Bus::new();
    let result = NuTool::execute(
        &serde_json::json!({"command": "exit 3"}),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap();
    assert_ne!(result["exit_code"], 0);
}

#[tokio::test]
async fn nu_empty_output_handled() {
    let bus = Bus::new();
    let result = NuTool::execute(
        &serde_json::json!({"command": "null"}),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap();
    assert_eq!(result["stdout"], "");
    assert_eq!(result["stderr"], "");
    assert_eq!(result["exit_code"], 0);
}

#[tokio::test]
async fn nu_handles_large_output_without_deadlock() {
    let bus = Bus::new();
    // Generate ~100KB of output — exceeds typical 64KB pipe buffer
    let result = NuTool::execute(
        &serde_json::json!({"command": "1..10000 | each { |_| 'xxxxxxxxxxxxxxxxxxxx' } | str join (char newline)"}),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap();
    assert_eq!(result["exit_code"], 0);
    let stdout = result["stdout"].as_str().unwrap();
    assert!(
        stdout.len() > 64_000,
        "expected >64KB output, got {} bytes",
        stdout.len()
    );
}

#[tokio::test]
async fn nu_timeout_kills_process_and_returns_error() {
    let bus = Bus::new();
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
async fn nu_cancellation_kills_process_quickly() {
    let bus = Bus::new();
    let bus2 = bus.clone();
    let handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = bus2.cancel().send(CancelEvent::Requested);
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
