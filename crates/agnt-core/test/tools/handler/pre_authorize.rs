use std::path::PathBuf;

use nu_protocol::Span;
use serde_json::json;

use super::*;
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
fn test_pre_authorize_tool_call_nu_carries_no_producer_preview() -> Result<()> {
    // -- Setup & Fixtures: a nu call whose command is in the arguments. The
    // code block a gate card shows is derived by the surface from that
    // command, so the producer owes no preview for it.
    let cases: Vec<(&str, serde_json::Value)> = vec![
        (
            "string command",
            json!({"command": "ls | where size > 1mb"}),
        ),
        ("missing command", json!({})),
        ("non-string command", json!({"command": 42})),
    ];

    // -- Exec & Check
    for (name, arguments) in cases {
        let tool_call = make_tool_call("nu", arguments)?;
        let output = pre_authorize_tool_call(
            &tool_call,
            ToolSource::Builtin,
            &TestEngine {
                cwd: PathBuf::from("/tmp"),
            },
        );

        assert!(
            output.preview.is_none(),
            "case '{name}': nu carries no producer preview"
        );
        assert!(
            output.ask_context.preview.is_none(),
            "case '{name}': the ask context carries none either"
        );
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
        output.preview.is_none(),
        "edit path behavior must stay unchanged (no preview for a non-applicable edit)"
    );
    assert!(output.ask_context.preview.is_none());
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
            output.preview.is_none(),
            "case '{name}': display must be None"
        );
        assert!(
            output.ask_context.preview.is_none(),
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
                let preview = output
                    .preview
                    .ok_or("a valid edit plan must produce the proposed diff")?;
                assert!(
                    preview.contains(removed) && preview.contains(added),
                    "case '{}': diff must contain {removed} and {added}, got: {preview:?}",
                    case.name
                );
                let ask_preview = output
                    .ask_context
                    .preview
                    .ok_or("edit apply must populate ask_context.preview")?;
                assert_eq!(
                    ask_preview, preview,
                    "case '{}': the ask context mirrors the preview",
                    case.name
                );
            }
            None => {
                assert!(
                    output.preview.is_none(),
                    "case '{}': no diff to approve; pre-authorize must produce no preview",
                    case.name
                );
                assert!(
                    output.ask_context.preview.is_none(),
                    "case '{}': ask_context must stay empty",
                    case.name
                );
            }
        }
    }
    Ok(())
}
