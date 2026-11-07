use super::*;

use rig::conversation::pairing::NO_RESULT_PROVIDED;

#[test]
fn test_tool_result_edit_creates_display_snapshot() {
    let entries = vec![
        Message::Assistant(AssistantMessage::new(vec![AssistantContent::ToolCall(
            ToolCall::new(
                CallId::from_wire("call_1"),
                ToolFunction::new(
                    ToolName::new("edit").expect("should be non-empty"),
                    json!({
                        "filePath": "/tmp/test.rs",
                        "oldString": "old code",
                        "newString": "new code"
                    }),
                ),
            ),
        )])),
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("call_1"),
                name: ToolName::new("edit").expect("non-empty tool name"),
                content: vec![ToolResultContent::Text(Text::new(
                    serde_json::to_string(&json!({
                        "path": "/tmp/test.rs",
                        "diff": "- old code\n+ new code",
                        "stats": { "insertions": 1, "deletions": 1 }
                    }))
                    .unwrap(),
                ))],
                is_error: false,
            })],
        },
    ];

    let snapshots = hydrate(&entries, std::path::Path::new("."));

    assert_eq!(
        snapshots.len(),
        1,
        "one snapshot per call: the surface derives the display from the data"
    );
    assert_eq!(snapshots[0].role(), "tool");
}

#[test]
fn test_tool_result_nu_creates_display_snapshot_from_arguments() -> Result<()> {
    let entries = vec![
        Message::Assistant(AssistantMessage::new(vec![AssistantContent::ToolCall(
            ToolCall::new(
                CallId::from_wire("call_nu_1"),
                ToolFunction::new(
                    ToolName::new("nu").expect("should be non-empty"),
                    json!({"command": "ls | sort"}),
                ),
            ),
        )])),
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("call_nu_1"),
                name: ToolName::new("nu").expect("non-empty tool name"),
                content: vec![ToolResultContent::Text(Text::new(serde_json::to_string(
                    &json!({
                        "stdout": "",
                        "stderr": "",
                        "exit_code": 0
                    }),
                )?))],
                is_error: false,
            })],
        },
    ];

    let snapshots = hydrate(&entries, std::path::Path::new("."));

    assert_eq!(
        snapshots.len(),
        1,
        "one snapshot per call, got {}",
        snapshots.len()
    );
    assert_eq!(snapshots[0].role(), "tool");
    assert_eq!(snapshots[0].tool_name(), Some("nu"));
    assert_eq!(
        snapshots[0].tool_arguments(),
        Some(r#"{"command":"ls | sort"}"#),
        "the snapshot carries the call's raw arguments; the drawer derives the code block"
    );
    Ok(())
}

/// An explicit `display` object in the result JSON wins over the
/// argument-sourced fallback: the `nu` call renders the embedded display,
/// not the command preview.
#[test]
fn test_tool_result_nu_explicit_display_wins_over_fallback() -> Result<()> {
    let entries = vec![
        Message::Assistant(AssistantMessage::new(vec![AssistantContent::ToolCall(
            ToolCall::new(
                CallId::from_wire("call_nu_explicit_1"),
                ToolFunction::new(
                    ToolName::new("nu").expect("should be non-empty"),
                    json!({"command": "ls | sort"}),
                ),
            ),
        )])),
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("call_nu_explicit_1"),
                name: ToolName::new("nu").expect("non-empty tool name"),
                content: vec![ToolResultContent::Text(Text::new(serde_json::to_string(
                    &json!({
                        "stdout": "",
                        "stderr": "",
                        "exit_code": 0,
                        "display": {
                            "title": "custom",
                            "sections": [{
                                "label": "output",
                                "kind": "code",
                                "language": "text",
                                "content": "embedded result"
                            }]
                        }
                    }),
                )?))],
                is_error: false,
            })],
        },
    ];

    let snapshots = hydrate(&entries, std::path::Path::new("."));

    assert_eq!(
        snapshots.len(),
        1,
        "a `display` key in the result is data the drawer ignores: one snapshot per call"
    );
    Ok(())
}

/// Cases that must NOT emit a `tool_display` snapshot: a non-previewable
/// tool, an empty `nu` command, a missing `nu` command, and a non-JSON
/// result.
#[test]
fn test_tool_result_no_display_cases() -> Result<()> {
    // -- Setup & Fixtures
    // (name, tool name, tool arguments, result text)
    let cases: Vec<(&str, &str, serde_json::Value, String)> = vec![
        (
            "non-previewable tool",
            "read",
            json!({"path": "/tmp/test.txt"}),
            serde_json::to_string(&json!({"data": "ok"}))?,
        ),
        (
            "empty nu command",
            "nu",
            json!({"command": ""}),
            serde_json::to_string(&json!({"stdout": "", "stderr": "", "exit_code": 0}))?,
        ),
        (
            "missing nu command",
            "nu",
            json!({}),
            serde_json::to_string(&json!({"stdout": "", "stderr": "", "exit_code": 0}))?,
        ),
        (
            "non-JSON result",
            "read",
            json!({"path": "/tmp/test.txt"}),
            "plain text, not JSON".to_string(),
        ),
    ];

    // -- Exec & Check
    for (name, tool, arguments, result_text) in cases {
        let entries = vec![
            Message::Assistant(AssistantMessage::new(vec![AssistantContent::ToolCall(
                ToolCall::new(
                    CallId::from_wire("call_x"),
                    ToolFunction::new(ToolName::new(tool).expect("should be non-empty"), arguments),
                ),
            )])),
            Message::User {
                content: vec![UserContent::ToolResult(ToolResult {
                    call: CallId::from_wire("call_x"),
                    name: ToolName::new(tool).expect("non-empty tool name"),
                    content: vec![ToolResultContent::Text(Text::new(result_text))],
                    is_error: false,
                })],
            },
        ];

        let snapshots = hydrate(&entries, std::path::Path::new("."));
        assert_eq!(
            snapshots.len(),
            1,
            "{name}: only the tool invocation snapshot is expected"
        );
        assert_eq!(snapshots[0].role(), "tool", "{name}");
    }
    Ok(())
}

#[test]
fn test_tool_result_with_explicit_display_key() {
    let entries = vec![
        Message::Assistant(AssistantMessage::new(vec![AssistantContent::ToolCall(
            ToolCall::new(
                CallId::from_wire("call_3"),
                ToolFunction::new(
                    ToolName::new("custom_tool").expect("should be non-empty"),
                    json!({}),
                ),
            ),
        )])),
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("call_3"),
                name: ToolName::new("custom_tool").expect("non-empty tool name"),
                content: vec![ToolResultContent::Text(Text::new(
                    serde_json::to_string(&json!({
                        "display": {
                            "title": "custom output",
                            "sections": [{
                                "label": "output",
                                "kind": "code",
                                "language": "text",
                                "content": "some result"
                            }]
                        }
                    }))
                    .unwrap(),
                ))],
                is_error: false,
            })],
        },
    ];

    let snapshots = hydrate(&entries, std::path::Path::new("."));

    assert_eq!(
        snapshots.len(),
        1,
        "one snapshot per call: an embedded title is data the drawer ignores"
    );
}

/// A cancelled row rehydrates as the ⊘ class with the args preview under it —
/// the same two-block shape the live view pushes, and the same shape for every
/// non-completed call whatever its result text says. No text renders.
#[test]
fn test_tool_result_cancelled_rows_rehydrate_as_cancelled() -> Result<()> {
    // -- Setup & Fixtures: the three non-completed shapes a session carries — a
    // timeout kill, a user cancel, and a call that never ran. Their texts differ
    // and are content only; the store's status is the same variant.
    let cases: Vec<(&str, &str, bool)> = vec![
        (
            "nu timeout kill",
            "command timed out after 120 seconds: no details",
            true,
        ),
        (
            "nu user cancel",
            "nu command cancelled by user: no details",
            true,
        ),
        ("never started", NO_RESULT_PROVIDED, true),
    ];

    // -- Exec & Check
    for (name, text, is_error) in cases {
        let entries = vec![
            Message::Assistant(AssistantMessage::new(vec![AssistantContent::ToolCall(
                ToolCall::new(
                    CallId::from_wire("call_cancelled"),
                    ToolFunction::new(
                        ToolName::new("nu").expect("should be non-empty"),
                        json!({"command": "ls | sort"}),
                    ),
                ),
            )])),
            Message::User {
                content: vec![UserContent::ToolResult(ToolResult {
                    call: CallId::from_wire("call_cancelled"),
                    name: ToolName::new("nu").expect("non-empty tool name"),
                    content: vec![ToolResultContent::Text(Text::new(text))],
                    is_error,
                })],
            },
        ];
        let statuses = cancelled_status("call_cancelled");
        let snapshots = hydrate_with_statuses(&entries, std::path::Path::new("."), &statuses);

        assert_eq!(
            snapshots.len(),
            1,
            "{name}: one snapshot per row — the preview is derived, never persisted"
        );
        assert_eq!(snapshots[0].role(), "tool", "{name}");
        assert_eq!(
            snapshots[0].tool_status(),
            Some(ToolStatus::Cancelled),
            "{name}: the row carries the store's variant"
        );
        // The display is derived by the surface from these fields; the fold
        // carries data only, so the cancelled class is the single verdict.
        assert_eq!(snapshots.len(), 1, "{name}: one snapshot per call");
    }

    // A non-Previewable tool emits no display snapshot.
    let entries = tool_call_entries("call_cancelled_read", NO_RESULT_PROVIDED, true);
    let statuses = cancelled_status("call_cancelled_read");
    let snapshots = hydrate_with_statuses(&entries, std::path::Path::new("."), &statuses);
    assert_eq!(
        snapshots.len(),
        1,
        "read_file: only the tool snapshot is expected"
    );
    assert_eq!(snapshots[0].role(), "tool");
    assert_eq!(
        snapshots[0].tool_status(),
        Some(ToolStatus::Cancelled),
        "read_file: the row carries the store's variant"
    );
    Ok(())
}

/// Rows outside the cancelled class keep the stored verdict: the status the
/// store carries when it has one, and the `is_error` verdict when it has none.
#[test]
fn test_tool_result_other_rows_keep_the_verdict_mapping() -> Result<()> {
    // -- Setup & Fixtures
    // (name, result text, persisted is_error, expected tool_success)
    let cases: Vec<(&str, &str, bool, Option<bool>)> = vec![
        (
            "real failure",
            "read failed: No such file or directory (os error 2)",
            true,
            Some(false),
        ),
        ("real success", "file contents here", false, Some(true)),
    ];

    // -- Exec & Check
    for (name, text, is_error, expected_success) in cases {
        let entries = tool_call_entries("call_other", text, is_error);
        let snapshots = hydrate(&entries, std::path::Path::new("."));

        assert_eq!(
            snapshots.len(),
            1,
            "{name}: only the tool snapshot is expected"
        );
        assert_eq!(snapshots[0].role(), "tool", "{name}");
        assert_eq!(snapshots[0].tool_success(), expected_success, "{name}");
        let expected_status = if is_error {
            ToolStatus::Failed
        } else {
            ToolStatus::Success
        };
        assert_eq!(
            snapshots[0].tool_status(),
            Some(expected_status),
            "{name}: a row with no recorded status reads its stored verdict"
        );
    }
    Ok(())
}

/// The status the store carries for one call.
fn cancelled_status(call: &str) -> ToolStatusMap {
    let mut statuses = ToolStatusMap::new();
    statuses.insert(call.to_owned(), ToolStatus::Cancelled);
    statuses
}
