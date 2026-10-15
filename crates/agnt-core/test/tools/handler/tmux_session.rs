use super::super::builtin_tool::BuiltinTool;
use super::super::tmux_common::parse_args;
use super::TmuxSessionTool;
use crate::bus::Bus;
use crate::tools::handler::ToolErrorKind;
use std::path::Path;

#[tokio::test]
async fn validation_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, serde_json::Value, Option<&str>)> = vec![
        ("missing action", serde_json::json!({}), None),
        (
            "unknown action",
            serde_json::json!({ "action": "bogus" }),
            Some("Unknown tmux_session action"),
        ),
        (
            "info requires session",
            serde_json::json!({ "action": "info" }),
            Some("requires 'session'"),
        ),
        (
            "create requires name",
            serde_json::json!({ "action": "create" }),
            Some("requires 'name'"),
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
        let err = TmuxSessionTool::execute(&args, Path::new("/tmp"), &bus)
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
