use super::*;

// --- tool_success rehydration tests ---

/// A row without a persisted verdict flag rehydrates as tool_success ==
/// Some(true) — output text is never consulted, whatever it looks like.
#[test]
fn hydrate_unflagged_tool_results_rehydrate_as_success() {
    // -- Setup & Fixtures
    // (name, call id, result text)
    let cases: Vec<(&str, &str, &str)> = vec![
        (
            "toolset error text",
            "call_fail_1",
            "Toolset error: command exited with code 1",
        ),
        ("plain success text", "call_ok_1", "file contents here"),
        ("permission denied text", "call_perm_1", "Permission denied"),
        (
            "doom loop text",
            "call_doom_1",
            "Doom loop detected: 'nu' called 5 times with identical arguments",
        ),
        (
            "tool quote text",
            "call_legacy_quote_1",
            "Tool 'nonexistent' is not available.",
        ),
        (
            "enriched denial text",
            "call_legacy_denial_1",
            "Permission denied by rule 'global:*' (scope: global)",
        ),
        (
            "unmatched failure text",
            "call_legacy_unmatched_1",
            "read failed: No such file or directory (os error 2)",
        ),
    ];

    // -- Exec & Check
    for (name, call_id, text) in cases {
        let entries = tool_call_entries(call_id, text, false);
        let snapshots = hydrate(&entries, std::path::Path::new("."));
        assert_eq!(snapshots.len(), 1, "{name}: one tool snapshot expected");
        assert_eq!(snapshots[0].role(), "tool", "{name}");
        assert_eq!(
            snapshots[0].tool_success(),
            Some(true),
            "{name}: unflagged rows must rehydrate as success — no text sniffing"
        );
    }
}

// --- multi-call display pairing tests ---

/// A display-yielding result pairs with its own call: with calls [A, B, C] in
/// one assistant message and only A yielding a display, the display snapshot
/// lands between A and B.
#[test]
fn hydrate_multi_call_turn_pairs_display_with_owning_call() {
    // -- Setup & Fixtures
    let entries = multi_call_entries(&[
        ("call_nu_a", "nu", json!({"command": "ls"})),
        ("call_get_b", "c5t__get_task", json!({"task_id": "b"})),
        ("call_get_c", "c5t__get_task", json!({"task_id": "c"})),
    ]);

    // -- Exec
    let snapshots = hydrate(&entries, std::path::Path::new("."));

    // -- Check
    let roles: Vec<&str> = snapshots.iter().map(|snapshot| snapshot.role()).collect();
    assert_eq!(
        roles,
        vec!["tool", "tool_display", "tool", "tool"],
        "the nu display must dock under its own call, before the get_task rows"
    );
    let display_content = snapshots[1]
        .tool_display()
        .and_then(|display| display.sections.first())
        .map(|section| section.content.as_str());
    assert_eq!(
        display_content,
        Some("ls"),
        "the display under the nu call must be its command preview"
    );
}

/// Every display-yielding call in one assistant message interleaves with its
/// own tool snapshot in call order.
#[test]
fn hydrate_multi_call_turn_interleaves_every_display() {
    // -- Setup & Fixtures
    let entries = multi_call_entries(&[
        ("call_nu_a", "nu", json!({"command": "ls"})),
        ("call_nu_b", "nu", json!({"command": "pwd"})),
        ("call_nu_c", "nu", json!({"command": "date"})),
    ]);

    // -- Exec
    let snapshots = hydrate(&entries, std::path::Path::new("."));

    // -- Check
    let roles: Vec<&str> = snapshots.iter().map(|snapshot| snapshot.role()).collect();
    assert_eq!(
        roles,
        vec![
            "tool",
            "tool_display",
            "tool",
            "tool_display",
            "tool",
            "tool_display"
        ],
        "each display must follow its own tool snapshot"
    );
    let display_contents: Vec<Option<&str>> = snapshots
        .iter()
        .filter(|snapshot| snapshot.role() == "tool_display")
        .map(|snapshot| {
            snapshot
                .tool_display()
                .and_then(|display| display.sections.first())
                .map(|section| section.content.as_str())
        })
        .collect();
    assert_eq!(
        display_contents,
        vec![Some("ls"), Some("pwd"), Some("date")],
        "displays must follow call order"
    );
}

/// A ToolResult whose call id has no matching ToolCall yields no display
/// snapshot. The payload embeds a valid `display` object so the pin bites: a
/// regression that derives and emits a display for an orphan result would
/// otherwise pass this test vacuously.
#[test]
fn hydrate_orphan_tool_result_emits_no_display() {
    // -- Setup & Fixtures
    let entries = vec![Message::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: CallId::from_wire("call_orphan"),
            name: ToolName::new("nu").expect("non-empty tool name"),
            content: vec![ToolResultContent::Text(Text::new(
                json!({
                    "stdout": "",
                    "stderr": "",
                    "exit_code": 0,
                    "display": {
                        "sections": [{"language": "", "content": "orphan display"}]
                    }
                })
                .to_string(),
            ))],
            is_error: false,
        })],
    }];

    // -- Exec
    let snapshots = hydrate(&entries, std::path::Path::new("."));

    // -- Check
    assert!(
        snapshots.is_empty(),
        "an orphan result must emit no snapshot, got: {snapshots:?}"
    );
}

/// A persisted verdict flag decides tool_success, regardless of the output
/// text.
#[test]
fn hydrate_flagged_tool_results_use_persisted_verdict() {
    // -- Case 1: nu_agent_success=true wins over failure-shaped text
    let entries = tool_call_entries(
        "call_flag_ok_1",
        "Tool 'nonexistent' is not available.",
        false,
    );
    let snapshots = hydrate(&entries, std::path::Path::new("."));
    assert_eq!(snapshots.len(), 1, "flag must not change snapshot count");
    assert_eq!(snapshots[0].role(), "tool");
    assert_eq!(
        snapshots[0].tool_success(),
        Some(true),
        "persisted nu_agent_success=true must win over the `Tool '` text sniff"
    );

    // -- Case 2: nu_agent_success=false rehydrates as failure
    let entries = tool_call_entries(
        "call_flag_fail_1",
        "read failed: No such file or directory (os error 2)",
        true,
    );
    let snapshots = hydrate(&entries, std::path::Path::new("."));
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");
    assert_eq!(
        snapshots[0].tool_success(),
        Some(false),
        "persisted nu_agent_success=false must rehydrate as failure"
    );
}

// -- Test Support

/// Build [assistant with one ToolCall per `calls` entry, user with one
/// ToolResult per entry]. Every result text is a JSON object, so the
/// result-sourced display path is reachable for each call.
fn multi_call_entries(calls: &[(&str, &str, serde_json::Value)]) -> Vec<Message> {
    let tool_calls = calls
        .iter()
        .map(|(call_id, tool_name, arguments)| {
            AssistantContent::ToolCall(ToolCall::new(
                CallId::from_wire(*call_id),
                ToolFunction::new(
                    ToolName::new(*tool_name).expect("should be non-empty"),
                    arguments.clone(),
                ),
            ))
        })
        .collect();
    let tool_results = calls
        .iter()
        .map(|(call_id, tool_name, _)| {
            UserContent::ToolResult(ToolResult {
                call: CallId::from_wire(*call_id),
                name: ToolName::new(*tool_name).expect("non-empty tool name"),
                content: vec![ToolResultContent::Text(Text::new(
                    json!({"stdout": "", "stderr": "", "exit_code": 0}).to_string(),
                ))],
                is_error: false,
            })
        })
        .collect();
    vec![
        Message::Assistant(AssistantMessage::new(tool_calls)),
        Message::User {
            content: tool_results,
        },
    ]
}
