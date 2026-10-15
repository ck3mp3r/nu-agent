use super::super::builtin_tool::BuiltinTool;
use super::super::tmux_common::parse_args;
use super::TmuxLayoutTool;
use crate::bus::Bus;
use crate::tools::handler::ToolErrorKind;
use std::path::Path;

#[tokio::test]
async fn validation_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, serde_json::Value, Option<&str>)> = vec![
        (
            "missing action",
            serde_json::json!({"session": "main", "window": "0", "layout": "tiled"}),
            None,
        ),
        (
            "unknown action",
            serde_json::json!({
                "action": "bogus",
                "session": "main",
                "window": "0",
                "layout": "tiled"
            }),
            Some("Unknown tmux_layout action"),
        ),
        (
            "missing layout",
            serde_json::json!({"action": "select", "session": "main", "window": "0"}),
            None,
        ),
    ];

    // -- Exec & Check
    let bus = Bus::default();
    for (name, args, expected_message) in cases {
        let err = TmuxLayoutTool::execute(&args, Path::new("/tmp"), &bus)
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
