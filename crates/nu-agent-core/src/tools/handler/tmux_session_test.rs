use super::super::builtin_tool::BuiltinTool;
use super::super::tmux_common::parse_args;
use super::TmuxSessionTool;
use crate::bus::Bus;
use crate::tools::handler::ToolErrorKind;
use std::path::Path;

#[tokio::test]
async fn missing_action_returns_validation_error() {
    let bus = Bus::new();
    let err = TmuxSessionTool::execute(&serde_json::json!({}), Path::new("/tmp"), &bus)
        .await
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
}

#[tokio::test]
async fn unknown_action_returns_validation_error() {
    let bus = Bus::new();
    let err = TmuxSessionTool::execute(
        &serde_json::json!({ "action": "bogus" }),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
    assert!(err.message.contains("Unknown tmux_session action"));
}

#[tokio::test]
async fn info_requires_session() {
    let bus = Bus::new();
    let err = TmuxSessionTool::execute(
        &serde_json::json!({ "action": "info" }),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
    assert!(err.message.contains("requires 'session'"));
}

#[tokio::test]
async fn create_requires_name() {
    let bus = Bus::new();
    let err = TmuxSessionTool::execute(
        &serde_json::json!({ "action": "create" }),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
    assert!(err.message.contains("requires 'name'"));
}

#[tokio::test]
async fn kill_requires_force() {
    let bus = Bus::new();
    let err = TmuxSessionTool::execute(
        &serde_json::json!({ "action": "kill", "session": "main" }),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
    assert!(err.message.contains("force: true"));
}

#[test]
fn session_args_deserialize() {
    let args: super::SessionArgs = parse_args(&serde_json::json!({
        "action": "create",
        "name": "work",
        "directory": "/abs",
        "force": true
    }))
    .unwrap();
    assert_eq!(args.action, "create");
    assert_eq!(args.name.as_deref(), Some("work"));
    assert_eq!(args.force, Some(true));
}
