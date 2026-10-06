use super::super::builtin_tool::BuiltinTool;
use super::super::tmux_common::parse_args;
use super::TmuxLayoutTool;
use crate::bus::Bus;
use crate::tools::handler::ToolErrorKind;
use std::path::Path;

#[tokio::test]
async fn missing_action_returns_validation_error() {
    let bus = Bus::default();
    let err = TmuxLayoutTool::execute(
        &serde_json::json!({"session": "main", "window": "0", "layout": "tiled"}),
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
    let err = TmuxLayoutTool::execute(
        &serde_json::json!({
            "action": "bogus",
            "session": "main",
            "window": "0",
            "layout": "tiled"
        }),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
    assert!(err.message.contains("Unknown tmux_layout action"));
}

#[tokio::test]
async fn missing_layout_returns_validation_error() {
    let bus = Bus::default();
    let err = TmuxLayoutTool::execute(
        &serde_json::json!({"action": "select", "session": "main", "window": "0"}),
        Path::new("/tmp"),
        &bus,
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
}

#[test]
fn layout_args_deserialize() {
    let args: super::LayoutArgs = parse_args(&serde_json::json!({
        "action": "select",
        "session": "main",
        "window": "2",
        "layout": "main-vertical"
    }))
    .unwrap();
    assert_eq!(args.action, "select");
    assert_eq!(args.session, "main");
    assert_eq!(args.window, "2");
    assert_eq!(args.layout, "main-vertical");
}
