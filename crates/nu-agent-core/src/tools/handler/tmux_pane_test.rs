use super::super::builtin_tool::BuiltinTool;
use super::super::tmux_common::parse_args;
use super::TmuxPaneTool;
use crate::bus::Bus;
use crate::tools::handler::ToolErrorKind;
use std::path::Path;

#[tokio::test]
async fn missing_action_returns_validation_error() {
    let bus = Bus::default();
    let err = TmuxPaneTool::execute(
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
    let err = TmuxPaneTool::execute(
        &serde_json::json!({ "action": "list" }),
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
    let err = TmuxPaneTool::execute(
        &serde_json::json!({ "action": "bogus", "session": "main" }),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
    assert!(err.message.contains("Unknown tmux_pane action"));
}

#[tokio::test]
async fn find_requires_name_or_context() {
    let bus = Bus::default();
    let err = TmuxPaneTool::execute(
        &serde_json::json!({ "action": "find", "session": "main" }),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
    assert!(err.message.contains("requires 'name' or 'context'"));
}

#[tokio::test]
async fn send_requires_command() {
    let bus = Bus::default();
    let err = TmuxPaneTool::execute(
        &serde_json::json!({ "action": "send", "session": "main" }),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
    assert!(err.message.contains("requires 'command'"));
}

#[tokio::test]
async fn split_requires_valid_direction() {
    let bus = Bus::default();
    let err = TmuxPaneTool::execute(
        &serde_json::json!({ "action": "split", "session": "main", "direction": "diagonal" }),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
    assert!(err.message.contains("expected 'horizontal' or 'vertical'"));
}

#[tokio::test]
async fn kill_requires_force() {
    let bus = Bus::default();
    let err = TmuxPaneTool::execute(
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
fn pane_args_deserialize() {
    let args: super::PaneArgs = parse_args(&serde_json::json!({
        "action": "split",
        "session": "main",
        "direction": "horizontal",
        "size": 50,
        "force": true
    }))
    .unwrap();
    assert_eq!(args.action, "split");
    assert_eq!(args.direction.as_deref(), Some("horizontal"));
    assert_eq!(args.size, Some(50));
    assert_eq!(args.force, Some(true));
}
