use super::super::builtin_tool::BuiltinTool;
use super::super::tmux_common::parse_args;
use super::TmuxWindowTool;
use crate::bus::Bus;
use crate::tools::handler::ToolErrorKind;
use std::path::Path;

#[tokio::test]
async fn missing_action_returns_validation_error() {
    let bus = Bus::default();
    let err = TmuxWindowTool::execute(
        &serde_json::json!({ "session": "main" }),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
}

#[tokio::test]
async fn missing_session_returns_validation_error() {
    let bus = Bus::default();
    let err = TmuxWindowTool::execute(
        &serde_json::json!({ "action": "create" }),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
}

#[tokio::test]
async fn unknown_action_returns_validation_error() {
    let bus = Bus::default();
    let err = TmuxWindowTool::execute(
        &serde_json::json!({ "action": "bogus", "session": "main" }),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
    assert!(err.message.contains("Unknown tmux_window action"));
}

#[tokio::test]
async fn kill_requires_force() {
    let bus = Bus::default();
    let err = TmuxWindowTool::execute(
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
fn window_args_deserialize() {
    let args: super::WindowArgs = parse_args(&serde_json::json!({
        "action": "create",
        "session": "main",
        "index": 3,
        "name": "editor",
        "force": false
    }))
    .unwrap();
    assert_eq!(args.action, "create");
    assert_eq!(args.session, "main");
    assert_eq!(args.index, Some(3));
    assert_eq!(args.force, Some(false));
}
