use super::super::builtin_tool::BuiltinTool;
use super::super::tmux_common::parse_args;
use super::TmuxPaneTool;
use crate::bus::Bus;
use crate::tools::handler::ToolErrorKind;
use std::path::Path;

#[tokio::test]
async fn validation_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, serde_json::Value, Option<&str>)> = vec![
        (
            "missing action",
            serde_json::json!({ "session": "main" }),
            None,
        ),
        (
            "missing session",
            serde_json::json!({ "action": "list" }),
            None,
        ),
        (
            "unknown action",
            serde_json::json!({ "action": "bogus", "session": "main" }),
            Some("Unknown tmux_pane action"),
        ),
        (
            "find requires name or context",
            serde_json::json!({ "action": "find", "session": "main" }),
            Some("requires 'name' or 'context'"),
        ),
        (
            "send requires command",
            serde_json::json!({ "action": "send", "session": "main" }),
            Some("requires 'command'"),
        ),
        (
            "split requires valid direction",
            serde_json::json!({
                "action": "split",
                "session": "main",
                "direction": "diagonal"
            }),
            Some("expected 'horizontal' or 'vertical'"),
        ),
        (
            "kill requires force",
            serde_json::json!({ "action": "kill", "session": "main" }),
            Some("force: true"),
        ),
    ];

    // -- Exec & Check
    let bus = Bus::default();
    for (name, args, expected_message) in cases {
        let err = TmuxPaneTool::execute(&args, Path::new("/tmp"), &bus)
            .await
            .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Validation, "case '{name}': kind");
        if let Some(message) = expected_message {
            assert!(
                err.message.contains(message),
                "case '{name}': message must contain '{message}', got: {}",
                err.message
            );
        }
    }
}

#[test]
fn pane_args_deserialize() {
    let args: super::PaneArgs = parse_args(&serde_json::json!({
        "action": "split",
        "session": "main",
        "window": "2",
        "direction": "horizontal",
        "size": 50,
        "force": true
    }))
    .unwrap();
    assert_eq!(args.action, "split");
    assert_eq!(args.window.as_deref(), Some("2"));
    assert_eq!(args.direction.as_deref(), Some("horizontal"));
    assert_eq!(args.size, Some(50));
    assert_eq!(args.force, Some(true));
}
