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
async fn nu_invalid_command_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, serde_json::Value)> = vec![
        ("missing command", serde_json::json!({})),
        ("non-string command", serde_json::json!({"command": 42})),
    ];

    // -- Exec & Check
    let bus = Bus::default();
    for (name, args) in cases {
        let err = NuTool::execute(&args, Path::new("/tmp"), &bus)
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
        &serde_json::json!({"command": "sleep 2sec", "timeout_seconds": 1}),
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
        // Publish once the tool has subscribed to the cancel channel: a
        // broadcast send succeeds only when a receiver is live. Bounded so a
        // missed subscription cannot hang the test.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if bus2.cancel().send(CancelEvent::Requested).await.is_ok() {
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
fn nu_preview_table() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, serde_json::Value, Option<&str>)> = vec![
        (
            "valid command returns a code display",
            serde_json::json!({"command": "ls | select name"}),
            Some("ls | select name"),
        ),
        ("empty command", serde_json::json!({"command": ""}), None),
        ("missing command", serde_json::json!({}), None),
    ];

    // -- Exec & Check
    for (name, args, expected_command) in cases {
        let display = NuTool::preview(&args, Path::new("/tmp"));
        match expected_command {
            Some(command) => {
                let display =
                    display.ok_or("preview must produce a display for a valid command")?;
                assert_eq!(display.title, None, "case '{name}': title");
                assert_eq!(display.sections.len(), 1, "case '{name}': section count");
                let section = &display.sections[0];
                assert!(
                    matches!(section.kind, ContentKind::Code { ref language } if language == "nu"),
                    "case '{name}': section kind must be Code(nu), got {:?}",
                    section.kind
                );
                assert_eq!(section.content, command, "case '{name}': section content");
                assert!(
                    section.stats.is_none(),
                    "case '{name}': code preview carries no stats"
                );
            }
            None => assert!(display.is_none(), "case '{name}': preview must return None"),
        }
    }
    Ok(())
}

// === call_line_render ===

#[test]
fn nu_call_line_render_table() -> Result<()> {
    // -- Setup & Fixtures
    // The command renders in the preview block, so the call line carries only
    // the applied timeout. Args without a usable command keep the generic
    // summary.
    let cases: Vec<(&str, &str, Option<&str>)> = vec![
        (
            "shows default timeout when omitted",
            r#"{"command":"ls"}"#,
            Some("⏱ 120s"),
        ),
        (
            "shows provided timeout",
            r#"{"command":"ls","timeout_seconds":60}"#,
            Some("⏱ 60s"),
        ),
        ("falls back to generic on invalid json", "not-json", None),
        (
            "falls back to generic on missing command",
            r#"{"timeout_seconds":5}"#,
            None,
        ),
        (
            "falls back to generic on empty command",
            r#"{"command":""}"#,
            None,
        ),
    ];

    // -- Exec & Check
    for (name, args, expected) in cases {
        let render = NuTool::call_line_render(args);
        match expected {
            Some(summary) => assert_eq!(
                render,
                CallLine {
                    summary: summary.to_string(),
                },
                "case '{name}'"
            ),
            None => assert_eq!(render, CallLine::from_json_summary(args), "case '{name}'"),
        }
    }
    Ok(())
}
