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
