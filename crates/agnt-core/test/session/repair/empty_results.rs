use super::*;

// ================================================================
// Gap 6 — fix_empty_tool_results tests
// ================================================================

/// `fix_empty_tool_results` replaces empty or whitespace-only ToolResult
/// content with `"(empty result)"` and logs a diagnostic naming the call id.
#[test]
fn fix_empty_tool_results_replaces_empty_content() {
    // -- Case 1: empty text
    let msgs = vec![user_with_content(vec![make_tool_result("tc1", "")])];
    let mut issues = Vec::new();
    let result = fix_empty_tool_results(msgs, &mut issues);

    assert_eq!(result.len(), 1);
    let Message::User { content } = &result[0] else {
        panic!("expected User message");
    };
    let tr = content
        .iter()
        .find_map(|c| match c {
            UserContent::ToolResult(tr) => Some(tr),
            _ => None,
        })
        .expect("ToolResult must still be present");
    let text: Vec<_> = tr
        .content
        .iter()
        .filter_map(|c| match c {
            crate::types::ToolResultContent::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        text,
        vec!["(empty result)"],
        "empty ToolResult content must be replaced with '(empty result)'"
    );
    assert!(
        issues.iter().any(|i| i.contains("tc1")),
        "diagnostic must reference tool result id=tc1; issues: {issues:?}"
    );

    // -- Case 2: whitespace-only text
    let msgs = vec![user_with_content(vec![make_tool_result("tc_ws", "   ")])];
    let mut issues = Vec::new();
    let result = fix_empty_tool_results(msgs, &mut issues);

    let Message::User { content } = &result[0] else {
        panic!("expected User message");
    };
    let tr = content
        .iter()
        .find_map(|c| match c {
            UserContent::ToolResult(tr) => Some(tr),
            _ => None,
        })
        .expect("ToolResult must still be present");
    let text: Vec<_> = tr
        .content
        .iter()
        .filter_map(|c| match c {
            crate::types::ToolResultContent::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        text,
        vec!["(empty result)"],
        "whitespace-only content must be treated as empty"
    );
    assert!(issues.iter().any(|i| i.contains("tc_ws")));
}

/// Non-empty ToolResult content and non-User messages are left untouched.
#[test]
fn fix_empty_tool_results_noop_when_content_present() {
    // -- Case 1: non-empty content is not modified
    let msgs = vec![user_with_content(vec![make_tool_result(
        "tc2",
        "actual output",
    )])];
    let original = msgs.clone();
    let mut issues = Vec::new();
    let result = fix_empty_tool_results(msgs, &mut issues);
    assert_msgs_eq(&result, &original);
    assert!(issues.is_empty(), "no issues for non-empty tool result");

    // -- Case 2: assistant and plain user messages are not modified
    let msgs = vec![Message::assistant("text"), Message::user("plain user text")];
    let original = msgs.clone();
    let mut issues = Vec::new();
    let result = fix_empty_tool_results(msgs, &mut issues);
    assert_msgs_eq(&result, &original);
    assert!(issues.is_empty());
}

/// The full `repair_messages` pipeline includes the empty ToolResult fix.
#[test]
fn repair_messages_pipeline_includes_empty_tool_result_fix() {
    let msgs = vec![
        Message::user("go"),
        assistant_with_content(vec![make_tool_call("tcX", "tool_x")]),
        user_with_content(vec![make_tool_result("tcX", "")]),
        Message::assistant("done"),
    ];
    let (result, issues) = repair_messages(msgs);

    let has_placeholder = result.iter().any(|msg| {
        let Message::User { content } = msg else {
            return false;
        };
        content.iter().any(|c| {
            match c {
            UserContent::ToolResult(tr) => tr.content.iter().any(|tc| {
                matches!(tc, crate::types::ToolResultContent::Text(t) if t.text == "(empty result)")
            }),
            _ => false,
        }
        })
    });
    assert!(
        has_placeholder,
        "repair_messages must replace empty tool result with '(empty result)'; result: {result:?}"
    );
    assert!(
        issues.iter().any(|i| i.contains("tcX")),
        "repair_messages must emit issue for empty tool result fix; issues: {issues:?}"
    );
}
