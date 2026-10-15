use std::path::PathBuf;

use nu_protocol::Span;
use serde_json::json;

use super::*;
use crate::transcript::ir::ContentKind;
use crate::types::{CallId, ToolCall, ToolFunction, ToolName};

// Test-support nu_plugin::EngineInterface implementation. The real
// EngineInterface cannot be constructed outside plugin context (see
// tools/executor_test.rs for the in-tree precedent), so tests supply their
// own `engine: EngineInterfaceLike` value. Production code is unaffected:
// `EngineInterfaceLike` is blanket-impl'd for the real `EngineInterface`
// and the functions take `impl` generics.
struct TestEngine {
    cwd: PathBuf,
}

impl EngineInterfaceLike for TestEngine {
    fn get_span_contents(&self, _span: Span) -> core::result::Result<Vec<u8>, String> {
        Err("span contents not needed by pre-authorize tests".to_string())
    }

    fn get_current_dir(&self) -> core::result::Result<String, String> {
        Ok(self.cwd.to_string_lossy().to_string())
    }
}

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn make_tool_call(name: &str, arguments: serde_json::Value) -> Result<ToolCall> {
    let tool_name = ToolName::new(name).map_err(|_| "tool name must not be empty")?;
    Ok(ToolCall::new(
        CallId::from_wire("test-id"),
        ToolFunction::new(tool_name, arguments),
    ))
}

#[test]
fn test_pre_authorize_tool_call_nu_table() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, serde_json::Value, Option<&str>)> = vec![
        (
            "string command produces a code preview",
            json!({"command": "ls | where size > 1mb"}),
            Some("ls | where size > 1mb"),
        ),
        ("missing command defaults", json!({}), None),
        ("non-string command defaults", json!({"command": 42}), None),
    ];

    // -- Exec & Check
    for (name, arguments, expected_command) in cases {
        let tool_call = make_tool_call("nu", arguments)?;
        let output = pre_authorize_tool_call(
            &tool_call,
            ToolSource::Builtin,
            &TestEngine {
                cwd: PathBuf::from("/tmp"),
            },
        );

        match expected_command {
            Some(command) => {
                let display = output
                    .display
                    .ok_or("nu with a command must produce a pre-authorize display")?;
                assert_eq!(display.title, None, "case '{name}': title");
                let section = display.sections.first().ok_or("should have one section")?;
                assert_eq!(
                    section.kind,
                    ContentKind::Code {
                        language: "nu".to_string()
                    },
                    "case '{name}': section kind"
                );
                assert_eq!(section.content, command, "case '{name}': section content");
                let ask_display = output
                    .ask_context
                    .pre_authorize_display
                    .ok_or("nu must populate ask_context.pre_authorize_display")?;
                assert_eq!(
                    ask_display, display,
                    "case '{name}': ask display mirrors display"
                );
            }
            None => {
                assert!(
                    output.display.is_none(),
                    "case '{name}': display must be None"
                );
                assert!(
                    output.ask_context.pre_authorize_display.is_none(),
                    "case '{name}': ask_context must stay empty"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn test_pre_authorize_tool_call_edit_path_unchanged_missing_command() -> Result<()> {
    // The edit path goes through pre_authorize_fs_tool, which returns None
    // when the arguments do not form a valid apply plan. These arguments
    // fail validation (search_replace without operation.search), so the
    // unchanged None → default path runs without touching disk.
    // -- Setup & Fixtures
    let tool_call = make_tool_call("edit", json!({"path": "nonexistent-file.txt"}))?;

    // -- Exec
    let output = pre_authorize_tool_call(
        &tool_call,
        ToolSource::Builtin,
        &TestEngine {
            cwd: PathBuf::from("/tmp"),
        },
    );

    // -- Check
    assert!(
        output.display.is_none(),
        "edit path behavior must stay unchanged (no preview for a non-applicable edit)"
    );
    assert!(output.ask_context.pre_authorize_display.is_none());
    Ok(())
}

#[test]
fn test_pre_authorize_tool_call_source_defaults_table() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        ("mcp source", ToolSource::Mcp),
        ("unknown source", ToolSource::Unknown),
    ];

    // -- Exec & Check
    for (name, source) in cases {
        let tool_call = make_tool_call("nu", json!({"command": "ls"}))?;
        let output = pre_authorize_tool_call(
            &tool_call,
            source,
            &TestEngine {
                cwd: PathBuf::from("/tmp"),
            },
        );
        assert!(
            output.display.is_none(),
            "case '{name}': display must be None"
        );
        assert!(
            output.ask_context.pre_authorize_display.is_none(),
            "case '{name}': ask_context must stay empty"
        );
    }
    Ok(())
}

#[test]
fn test_pre_authorize_fs_tool_edit_preview_table() -> Result<()> {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        seed: Option<&'static str>,
        build_args: fn(&std::path::Path) -> serde_json::Value,
        expected_diff: Option<(&'static str, &'static str)>,
    }

    let cases = vec![
        Case {
            name: "create apply produces a diff preview",
            seed: None,
            build_args: |target| {
                json!({
                    "path": target.to_string_lossy(),
                    "operation": {"type": "create", "content": "hello\n"}
                })
            },
            expected_diff: Some(("+hello", "+hello")),
        },
        Case {
            name: "search_replace without expected_version produces a diff preview",
            seed: Some("hello\nworld\n"),
            build_args: |target| {
                json!({
                    "path": target.to_string_lossy(),
                    "mode": "apply",
                    "operation": {
                        "type": "search_replace",
                        "search": "world",
                        "replacement": "there"
                    }
                })
            },
            expected_diff: Some(("-world", "+there")),
        },
        Case {
            name: "stale expected_version produces no preview",
            seed: Some("hello\nworld\n"),
            build_args: |target| {
                json!({
                    "path": target.to_string_lossy(),
                    "mode": "apply",
                    "expected_version": "deadbeef",
                    "operation": {
                        "type": "search_replace",
                        "search": "world",
                        "replacement": "there"
                    }
                })
            },
            expected_diff: None,
        },
        Case {
            name: "noop produces no preview",
            seed: Some("hello\nworld\n"),
            build_args: |target| {
                json!({
                    "path": target.to_string_lossy(),
                    "mode": "apply",
                    "operation": {
                        "type": "search_replace",
                        "search": "world",
                        "replacement": "world"
                    }
                })
            },
            expected_diff: None,
        },
    ];

    // -- Exec & Check
    for case in cases {
        let tmp = tempfile::tempdir()?;
        let target = tmp.path().join("target.txt");
        if let Some(seed) = case.seed {
            std::fs::write(&target, seed)?;
        }
        let tool_call = make_tool_call("edit", (case.build_args)(&target))?;

        let output = pre_authorize_tool_call(
            &tool_call,
            ToolSource::Builtin,
            &TestEngine {
                cwd: tmp.path().to_path_buf(),
            },
        );

        match case.expected_diff {
            Some((removed, added)) => {
                let display = output
                    .display
                    .ok_or("a valid edit plan must produce a pre-authorize display")?;
                let section = display.sections.first().ok_or("should have one section")?;
                assert!(
                    matches!(section.kind, ContentKind::Diff { .. }),
                    "case '{}': section kind must be Diff, got {:?}",
                    case.name,
                    section.kind
                );
                assert!(
                    section.content.contains(removed) && section.content.contains(added),
                    "case '{}': diff must contain {removed} and {added}, got: {:?}",
                    case.name,
                    section.content
                );
                let ask_display = output
                    .ask_context
                    .pre_authorize_display
                    .ok_or("edit apply must populate ask_context.pre_authorize_display")?;
                assert_eq!(
                    ask_display, display,
                    "case '{}': ask display mirrors display",
                    case.name
                );
            }
            None => {
                assert!(
                    output.display.is_none(),
                    "case '{}': no diff to approve; pre-authorize must produce no display",
                    case.name
                );
                assert!(
                    output.ask_context.pre_authorize_display.is_none(),
                    "case '{}': ask_context must stay empty",
                    case.name
                );
            }
        }
    }
    Ok(())
}
