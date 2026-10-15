use super::*;

// ================================================================
// Repair passes preserve tool-result integrity
// ================================================================

/// `trim_trailing_user` must NOT pop a User message that contains a
/// ToolResult, and `fix_tool_call_integrity` must strip a ToolCall/ToolResult
/// pair that is not adjacent.
#[test]
fn repair_preserves_tool_result_and_strips_non_adjacent_pair() {
    // -- Case 1: a trailing User(ToolResult) is not trimmed
    let msgs = vec![
        Message::user("prompt"),
        assistant_with_content(vec![make_tool_call("abc", "do_thing")]),
        user_with_content(vec![make_tool_result("abc", "ok")]),
    ];
    let (result, _) = repair_messages(msgs);
    let has_tool_result = result.iter().any(|msg| match msg {
        Message::User { content } => content
            .iter()
            .any(|i| matches!(i, UserContent::ToolResult(_))),
        _ => false,
    });
    assert!(
        has_tool_result,
        "User(ToolResult) must NOT be trimmed by trim_trailing_user"
    );

    // -- Case 2: a non-adjacent ToolCall/ToolResult pair is stripped
    let msgs = vec![
        Message::user("start"),
        assistant_with_content(vec![make_tool_call("abc", "do_thing")]),
        Message::user("in between"),
        user_with_content(vec![make_tool_result("abc", "ok")]),
    ];
    let (result, _) = repair_messages(msgs);

    let has_tool_call = result.iter().any(|msg| match msg {
        Message::Assistant(assistant) => assistant
            .content
            .iter()
            .any(|i| matches!(i, AssistantContent::ToolCall(_))),
        _ => false,
    });
    let has_tool_result = result.iter().any(|msg| match msg {
        Message::User { content } => content
            .iter()
            .any(|i| matches!(i, UserContent::ToolResult(_))),
        _ => false,
    });
    assert!(
        !has_tool_call,
        "non-adjacent ToolCall must be stripped from result"
    );
    assert!(
        !has_tool_result,
        "non-adjacent ToolResult must be stripped from result"
    );
}

/// `merge_consecutive_same_role` must NOT merge two consecutive Assistant
/// messages when either contains a ToolCall.
#[test]
fn merge_consecutive_same_role_does_not_merge_tool_call_assistants() {
    let msgs = vec![
        Message::user("q"),
        assistant_with_content(vec![make_tool_call("a", "tool_a")]),
        assistant_with_content(vec![make_tool_call("b", "tool_b")]),
        user_with_content(vec![make_tool_result("a", "res_a")]),
        user_with_content(vec![make_tool_result("b", "res_b")]),
    ];
    let mut issues = Vec::new();
    let result = merge_consecutive_same_role(msgs, &mut issues);

    let assistant_count = result
        .iter()
        .filter(|msg| matches!(msg, Message::Assistant(_)))
        .count();
    assert_eq!(
        assistant_count, 2,
        "two ToolCall-bearing Assistant messages must NOT be merged into one"
    );
    assert!(issues.is_empty(), "no merge diagnostic should be emitted");
}

// ================================================================
// inject_assistant_after_dangling_tool_results
// ================================================================

/// A synthetic assistant message is injected between a User(ToolResult) and a
/// following User(Text); valid and already-healed histories are untouched.
#[test]
fn inject_assistant_after_dangling_tool_results_heals() {
    // -- Case 1: the corrupt pattern is healed
    let msgs = vec![
        Message::user("prompt"),
        assistant_with_content(vec![make_tool_call("tc1", "do_thing")]),
        user_with_content(vec![make_tool_result("tc1", "output")]),
        Message::user("continue"),
    ];
    let mut issues = Vec::new();
    let result = inject_assistant_after_dangling_tool_results(msgs, &mut issues);
    assert_eq!(
        result.len(),
        5,
        "synthetic assistant must be injected between user(ToolResult) and user(Text)"
    );
    match &result[3] {
        Message::Assistant(assistant) => {
            let content = &assistant.content;
            let text = content.iter().find_map(|c| match c {
                crate::types::AssistantContent::Text(t) => Some(t.text.as_str()),
                _ => None,
            });
            assert!(
                text.is_some_and(|t| t.contains("[interrupted")),
                "injected assistant must contain '[interrupted'; got: {text:?}"
            );
        }
        _ => panic!("result[3] must be an Assistant message"),
    }
    assert!(
        issues
            .iter()
            .any(|i| i.contains("injected synthetic assistant after dangling tool results")),
        "must emit the expected diagnostic; issues: {issues:?}"
    );

    // -- Case 2: a valid history is unchanged
    let msgs = vec![Message::user("p"), Message::assistant("ok")];
    let expected = msgs.clone();
    let mut issues = Vec::new();
    let result = inject_assistant_after_dangling_tool_results(msgs, &mut issues);
    assert_msgs_eq(&result, &expected);
    assert!(issues.is_empty(), "no issues for valid input");

    // -- Case 3: an already-healed history is unchanged
    let msgs = vec![
        Message::user("p"),
        assistant_with_content(vec![make_tool_call("tc1", "do_thing")]),
        user_with_content(vec![make_tool_result("tc1", "x")]),
        Message::assistant("[Turn failed: server error]"),
        Message::user("continue"),
    ];
    let expected = msgs.clone();
    let mut issues = Vec::new();
    let result = inject_assistant_after_dangling_tool_results(msgs, &mut issues);
    assert_msgs_eq(&result, &expected);
    assert!(
        issues.is_empty(),
        "already-healed session must produce no issues; issues: {issues:?}"
    );

    // -- Case 4: the pass is idempotent
    let msgs = vec![
        Message::user("prompt"),
        assistant_with_content(vec![make_tool_call("tc1", "do_thing")]),
        user_with_content(vec![make_tool_result("tc1", "output")]),
        Message::user("continue"),
    ];
    let mut issues1 = Vec::new();
    let healed = inject_assistant_after_dangling_tool_results(msgs, &mut issues1);
    assert!(!issues1.is_empty(), "first pass must emit a diagnostic");

    let healed_clone = healed.clone();
    let mut issues2 = Vec::new();
    let healed_again = inject_assistant_after_dangling_tool_results(healed, &mut issues2);
    assert_msgs_eq(&healed_again, &healed_clone);
    assert!(
        issues2.is_empty(),
        "second pass must emit no issues (idempotent); issues: {issues2:?}"
    );
}
