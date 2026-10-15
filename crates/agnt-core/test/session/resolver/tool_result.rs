use super::*;

#[test]
fn test_tool_result_edit_creates_display_snapshot() {
    let entries = vec![
        StoreEntry::Message(Message::Assistant(AssistantMessage::new(vec![
            AssistantContent::ToolCall(ToolCall::new(
                CallId::from_wire("call_1"),
                ToolFunction::new(
                    ToolName::new("edit").expect("should be non-empty"),
                    json!({
                        "filePath": "/tmp/test.rs",
                        "oldString": "old code",
                        "newString": "new code"
                    }),
                ),
            )),
        ]))),
        StoreEntry::Message(Message::User {
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
        }),
    ];

    let snapshots =
        super::resolver::hydrate_transcript_from_store_entries(&entries, std::path::Path::new("."));

    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].role(), "tool");

    let display = snapshots[1].tool_display();
    assert!(
        display.is_some(),
        "Expected tool_display on second snapshot"
    );
    let display = display.unwrap();
    assert_eq!(display.title, None);
    assert_eq!(display.sections.len(), 1);
    assert!(
        matches!(
            display.sections[0].kind,
            crate::transcript::ir::ContentKind::Diff { .. }
        ),
        "section kind must be Diff, got {:?}",
        display.sections[0].kind
    );
}

#[test]
fn test_tool_result_nu_creates_display_snapshot_from_arguments() -> Result<()> {
    let entries = vec![
        StoreEntry::Message(Message::Assistant(AssistantMessage::new(vec![
            AssistantContent::ToolCall(ToolCall::new(
                CallId::from_wire("call_nu_1"),
                ToolFunction::new(
                    ToolName::new("nu").expect("should be non-empty"),
                    json!({"command": "ls | sort"}),
                ),
            )),
        ]))),
        StoreEntry::Message(Message::User {
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
        }),
    ];

    let snapshots =
        super::resolver::hydrate_transcript_from_store_entries(&entries, std::path::Path::new("."));

    assert_eq!(
        snapshots.len(),
        2,
        "expected tool call + tool_display snapshots, got {}",
        snapshots.len()
    );
    assert_eq!(snapshots[0].role(), "tool");
    assert_eq!(snapshots[1].role(), "tool_display");

    let display = snapshots[1]
        .tool_display()
        .ok_or("should be tool_display on second snapshot")?;
    assert_eq!(display.title, None);
    assert_eq!(display.sections.len(), 1);
    assert!(
        matches!(
            display.sections[0].kind,
            crate::transcript::ir::ContentKind::Code { ref language } if language == "nu"
        ),
        "section kind must be Code with language nu, got {:?}",
        display.sections[0].kind
    );
    assert_eq!(display.sections[0].content, "ls | sort");
    Ok(())
}

/// An explicit `display` object in the result JSON wins over the
/// argument-sourced fallback: the `nu` call renders the embedded display,
/// not the command preview.
#[test]
fn test_tool_result_nu_explicit_display_wins_over_fallback() -> Result<()> {
    let entries = vec![
        StoreEntry::Message(Message::Assistant(AssistantMessage::new(vec![
            AssistantContent::ToolCall(ToolCall::new(
                CallId::from_wire("call_nu_explicit_1"),
                ToolFunction::new(
                    ToolName::new("nu").expect("should be non-empty"),
                    json!({"command": "ls | sort"}),
                ),
            )),
        ]))),
        StoreEntry::Message(Message::User {
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
                                "language": "text",
                                "content": "embedded result"
                            }]
                        }
                    }),
                )?))],
                is_error: false,
            })],
        }),
    ];

    let snapshots =
        super::resolver::hydrate_transcript_from_store_entries(&entries, std::path::Path::new("."));

    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[1].role(), "tool_display");

    let display = snapshots[1]
        .tool_display()
        .ok_or("should be tool_display on second snapshot")?;
    assert_eq!(
        display.title,
        Some("custom".to_string()),
        "explicit display must win over the argument-sourced fallback"
    );
    assert_eq!(display.sections[0].content, "embedded result");
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
            StoreEntry::Message(Message::Assistant(AssistantMessage::new(vec![
                AssistantContent::ToolCall(ToolCall::new(
                    CallId::from_wire("call_x"),
                    ToolFunction::new(ToolName::new(tool).expect("should be non-empty"), arguments),
                )),
            ]))),
            StoreEntry::Message(Message::User {
                content: vec![UserContent::ToolResult(ToolResult {
                    call: CallId::from_wire("call_x"),
                    name: ToolName::new(tool).expect("non-empty tool name"),
                    content: vec![ToolResultContent::Text(Text::new(result_text))],
                    is_error: false,
                })],
            }),
        ];

        let snapshots = super::resolver::hydrate_transcript_from_store_entries(
            &entries,
            std::path::Path::new("."),
        );
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
        StoreEntry::Message(Message::Assistant(AssistantMessage::new(vec![
            AssistantContent::ToolCall(ToolCall::new(
                CallId::from_wire("call_3"),
                ToolFunction::new(
                    ToolName::new("custom_tool").expect("should be non-empty"),
                    json!({}),
                ),
            )),
        ]))),
        StoreEntry::Message(Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("call_3"),
                name: ToolName::new("custom_tool").expect("non-empty tool name"),
                content: vec![ToolResultContent::Text(Text::new(
                    serde_json::to_string(&json!({
                        "display": {
                            "title": "custom output",
                            "sections": [{
                                "label": "output",
                                "language": "text",
                                "content": "some result"
                            }]
                        }
                    }))
                    .unwrap(),
                ))],
                is_error: false,
            })],
        }),
    ];

    let snapshots =
        super::resolver::hydrate_transcript_from_store_entries(&entries, std::path::Path::new("."));

    assert_eq!(snapshots.len(), 2);

    let display = snapshots[1].tool_display();
    assert!(
        display.is_some(),
        "Expected tool_display on second snapshot"
    );
    let display = display.unwrap();
    assert_eq!(display.title, Some("custom output".to_string()));
}
