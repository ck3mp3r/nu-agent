use super::*;

// ================================================================
// fix_invalid_tool_arguments tests
// ================================================================

/// `fix_invalid_tool_arguments` resets invalid ToolCall arguments to `{}`,
/// clears `invalid_arguments`, and leaves valid arguments alone.
#[test]
fn fix_invalid_tool_arguments_resets_and_preserves() {
    // -- Case 1: invalid arguments are cleared
    let msgs = vec![
        Message::user("prompt"),
        assistant_with_content(vec![make_invalid_args_tool_call(
            "tc1",
            "tmux__send_and_capture",
        )]),
        user_with_content(vec![make_tool_result("tc1", "Tool not available")]),
    ];
    let mut issues = Vec::new();
    let result = fix_invalid_tool_arguments(msgs, &mut issues);

    match &result[1] {
        Message::Assistant(assistant) => {
            let content = &assistant.content;
            let tc = content.iter().find_map(|c| match c {
                AssistantContent::ToolCall(tc) => Some(tc),
                _ => None,
            });
            let tc = tc.expect("ToolCall must still be present");
            assert!(
                tc.function.arguments.is_empty(),
                "invalid arguments must be reset to {{}}"
            );
            assert!(
                tc.function.invalid_arguments.is_none(),
                "invalid_arguments must be cleared"
            );
        }
        _ => panic!("expected Assistant message at index 1"),
    }
    assert!(
        issues
            .iter()
            .any(|i| i.contains("cleared invalid tool call arguments")),
        "must emit diagnostic; issues: {issues:?}"
    );

    // -- Case 2: valid arguments are untouched
    let msgs = vec![
        Message::user("prompt"),
        assistant_with_content(vec![make_tool_call("tc1", "nu__shell")]),
        user_with_content(vec![make_tool_result("tc1", "file1")]),
    ];
    let original = msgs.clone();
    let mut issues = Vec::new();
    let result = fix_invalid_tool_arguments(msgs, &mut issues);
    assert_msgs_eq(&result, &original);
    assert!(issues.is_empty(), "no issues for valid args");

    // -- Case 3: the full pipeline heals invalid args end to end
    let msgs = vec![
        Message::user("prompt"),
        assistant_with_content(vec![make_invalid_args_tool_call(
            "tc1",
            "tmux__send_and_capture",
        )]),
        user_with_content(vec![make_tool_result("tc1", "Tool not available")]),
        Message::assistant("ok"),
    ];
    let (result, issues) = repair_messages(msgs);

    let tc = result.iter().find_map(|msg| match msg {
        Message::Assistant(assistant) => assistant.content.iter().find_map(|c| match c {
            AssistantContent::ToolCall(tc) => Some(tc),
            _ => None,
        }),
        _ => None,
    });
    let tc = tc.expect("ToolCall must still be present");
    assert!(
        tc.function.arguments.is_empty(),
        "repair_messages must reset invalid ToolCall arguments; result: {result:?}"
    );
    assert!(
        tc.function.invalid_arguments.is_none(),
        "repair_messages must clear invalid_arguments; result: {result:?}"
    );
    assert!(
        issues
            .iter()
            .any(|i| i.contains("cleared invalid tool call arguments")),
        "must emit invalid-args diagnostic; issues: {issues:?}"
    );
}

// ================================================================
// inject_missing_tool_results — call_id propagation
// ================================================================

/// A synthetic `ToolResult` produced for an unpaired `ToolCall` carries the
/// call id of its `ToolCall`; an empty wire id is replaced by a rig-issued
/// local id.
#[test]
fn inject_missing_tool_results_call_id() {
    // -- Case 1: the call id is preserved
    let tc = AssistantContent::ToolCall(ToolCall::new(
        CallId::from_wire("call_abc123"),
        ToolFunction::new(
            ToolName::new("do_thing").expect("non-empty tool name"),
            serde_json::json!({}),
        ),
    ));
    let msgs = vec![Message::user("go"), assistant_with_content(vec![tc])];
    let result = inject_missing_tool_results(msgs);

    assert_eq!(result.len(), 3, "synthetic ToolResult must be injected");
    match &result[2] {
        Message::User { content } => {
            let tr = content
                .iter()
                .find_map(|item| match item {
                    UserContent::ToolResult(tr) => Some(tr),
                    _ => None,
                })
                .expect("injected User message must contain a ToolResult");
            assert_eq!(
                tr.call.wire().as_ref(),
                "call_abc123",
                "synthetic ToolResult must carry the call id from its ToolCall"
            );
        }
        _ => panic!("message[2] must be a User message"),
    }

    // -- Case 2: an empty wire id is replaced by a local id
    let tc = AssistantContent::ToolCall(ToolCall::new(
        CallId::from_wire(""),
        ToolFunction::new(
            ToolName::new("do_other").expect("non-empty tool name"),
            serde_json::json!({}),
        ),
    ));
    let msgs = vec![Message::user("go"), assistant_with_content(vec![tc])];
    let result = inject_missing_tool_results(msgs);

    assert_eq!(result.len(), 3, "synthetic ToolResult must be injected");
    match &result[2] {
        Message::User { content } => {
            let tr = content
                .iter()
                .find_map(|item| match item {
                    UserContent::ToolResult(tr) => Some(tr),
                    _ => None,
                })
                .expect("injected User message must contain a ToolResult");
            assert!(
                tr.call.is_local(),
                "an empty wire id must be replaced by a rig-issued local id"
            );
        }
        _ => panic!("message[2] must be a User message"),
    }
}
