use super::super::builtin_tool::BuiltinTool;
use super::super::tmux_common::parse_args;
use super::{TmuxWindowTool, parse_window_create_output};
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
            serde_json::json!({ "action": "create" }),
            None,
        ),
        (
            "unknown action",
            serde_json::json!({ "action": "bogus", "session": "main" }),
            Some("Unknown tmux_window action"),
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
        let err = TmuxWindowTool::execute(&args, Path::new("/tmp"), &bus)
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

#[test]
fn parse_window_create_output_table() -> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures
    type Case = (&'static str, &'static str, Option<(&'static str, u64)>);

    let cases: Vec<Case> = vec![
        ("parses id and index", "@5:2", Some(("@5", 2))),
        ("malformed output", "not-a-valid-shape", None),
    ];

    // -- Exec & Check
    for (name, output, expected) in cases {
        let parsed = parse_window_create_output(output);
        match expected {
            Some((id, index)) => {
                let (actual_id, actual_index) = parsed.ok_or("a well-formed output must parse")?;
                assert_eq!(actual_id, id, "case '{name}': id");
                assert_eq!(actual_index, index, "case '{name}': index");
            }
            None => assert_eq!(parsed, None, "case '{name}': malformed output"),
        }
    }
    Ok(())
}
