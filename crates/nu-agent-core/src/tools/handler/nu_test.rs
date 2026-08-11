use super::{NuArgs, build_result, parse_args};
use crate::tools::handler::ToolErrorKind;
use std::path::Path;

#[test]
fn parse_args_parses_command() {
    let args: NuArgs = parse_args(&serde_json::json!({"command": "ls"})).unwrap();
    assert_eq!(args.command, "ls");
    assert!(args.timeout_seconds.is_none());
}

#[test]
fn parse_args_parses_timeout() {
    let args: NuArgs =
        parse_args(&serde_json::json!({"command": "ls", "timeout_seconds": 10})).unwrap();
    assert_eq!(args.command, "ls");
    assert_eq!(args.timeout_seconds, Some(10));
}

#[test]
fn parse_args_missing_command_is_validation_error() {
    let err = parse_args(&serde_json::json!({})).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
    assert!(
        err.message.contains("Invalid nu arguments"),
        "message: {}",
        err.message
    );
}

#[test]
fn parse_args_wrong_type_is_validation_error() {
    let err = parse_args(&serde_json::json!({"command": 42})).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
}

#[test]
fn build_result_formats_stdout_stderr_exit_code() {
    let result = build_result("hello".to_string(), "warn".to_string(), 0);
    assert_eq!(result["stdout"], "hello");
    assert_eq!(result["stderr"], "warn");
    assert_eq!(result["exit_code"], 0);
}

#[test]
fn build_result_preserves_nonzero_exit_code() {
    let result = build_result(String::new(), "boom".to_string(), 1);
    assert_eq!(result["stdout"], "");
    assert_eq!(result["stderr"], "boom");
    assert_eq!(result["exit_code"], 1);
}

#[test]
fn build_result_handles_empty_output() {
    let result = build_result(String::new(), String::new(), 0);
    assert_eq!(result["stdout"], "");
    assert_eq!(result["stderr"], "");
    assert_eq!(result["exit_code"], 0);
}

#[test]
fn dispatch_unknown_tool_returns_none() {
    let result = super::dispatch_nu_tool(
        "not_nu",
        &serde_json::json!({"command": "ls"}),
        Path::new("/tmp"),
    )
    .unwrap();
    assert!(result.is_none());
}
