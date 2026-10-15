use super::*;

// ================================================================
// Pass 1 — remove_empty_messages
// ================================================================

/// Structurally empty user and assistant messages are removed with a
/// diagnostic; non-empty messages are preserved.
#[test]
fn remove_empty_messages_removes_empty_and_preserves_valid() {
    // -- Case 1: user message with empty text is removed
    // Vec<T> can hold zero items, so "structurally empty" is impossible to
    // distinguish by type alone. The only user-empty case is all-empty text
    // with no ToolResults.
    let msg = user_with_content(vec![UserContent::Text(crate::types::Text::new(""))]);
    let mut issues = Vec::new();
    let result = remove_empty_messages(vec![msg], &mut issues);
    assert!(
        result.is_empty(),
        "user message with empty text must be removed"
    );
    assert!(!issues.is_empty(), "should emit a diagnostic");

    // -- Case 2: assistant message with whitespace-only text is removed
    let msg = assistant_with_content(vec![AssistantContent::text("   ")]);
    let mut issues = Vec::new();
    let result = remove_empty_messages(vec![msg], &mut issues);
    assert!(
        result.is_empty(),
        "assistant message with whitespace-only text must be removed"
    );
    assert!(!issues.is_empty());

    // -- Case 3: assistant message with empty text is removed
    let msg = assistant_with_content(vec![AssistantContent::text("")]);
    let mut issues = Vec::new();
    let result = remove_empty_messages(vec![msg], &mut issues);
    assert!(
        result.is_empty(),
        "assistant message with empty text must be removed"
    );
    assert!(!issues.is_empty());

    // -- Case 4: non-empty messages are preserved
    let msgs = vec![
        Message::user("hello"),
        Message::assistant("world"),
        Message::system("sys"),
    ];
    let mut issues = Vec::new();
    let result = remove_empty_messages(msgs.clone(), &mut issues);
    assert_msgs_eq(&result, &msgs);
    assert!(issues.is_empty(), "no issues for valid messages");
}

// ================================================================
// Pass 2 — fix_tool_call_integrity
// ================================================================

/// A ToolCall without a matching ToolResult and a ToolResult without a
/// matching ToolCall are both removed.
#[test]
fn fix_tool_call_integrity_removes_dangling_and_orphans() {
    // -- Case 1: dangling ToolCall (no result) removes the assistant message
    let msgs = vec![
        Message::user("hi"),
        assistant_with_content(vec![make_tool_call("X", "do_thing")]),
    ];
    let mut issues = Vec::new();
    let result = fix_tool_call_integrity(msgs, &mut issues);
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], Message::User { .. }));
    assert!(!issues.is_empty());

    // -- Case 2: orphaned ToolResult (no call) removes the user message
    let msgs = vec![
        Message::user("hi"),
        user_with_content(vec![make_tool_result("Z", "some result")]),
    ];
    let mut issues = Vec::new();
    let result = fix_tool_call_integrity(msgs, &mut issues);
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], Message::User { .. }));
    assert!(!issues.is_empty());
}

#[test]
fn fix_tool_call_integrity_preserves_matched_calls() {
    let msgs = vec![
        Message::user("hi"),
        assistant_with_content(vec![make_tool_call("X", "do_thing")]),
        user_with_content(vec![make_tool_result("X", "ok")]),
    ];
    let expected = msgs.clone();
    let mut issues = Vec::new();
    let result = fix_tool_call_integrity(msgs, &mut issues);
    assert_msgs_eq(&result, &expected);
    assert!(issues.is_empty());
}

/// Text alongside a dangling ToolCall survives, and a partial match keeps
/// only the calls that have results.
#[test]
fn fix_tool_call_integrity_keeps_text_and_partial_matches() {
    // -- Case 1: text is preserved when the ToolCall is removed
    let msgs = vec![
        Message::user("hi"),
        assistant_with_content(vec![
            AssistantContent::text("hello"),
            make_tool_call("X", "do_thing"),
        ]),
    ];
    let mut issues = Vec::new();
    let result = fix_tool_call_integrity(msgs, &mut issues);
    assert_eq!(result.len(), 2);
    match &result[1] {
        Message::Assistant(assistant) => {
            let content = &assistant.content;
            assert_eq!(content.len(), 1);
            assert!(matches!(content.first(), Some(AssistantContent::Text(_))));
        }
        _ => panic!("expected assistant message"),
    }
    assert!(!issues.is_empty());

    // -- Case 2: 3 tool calls (A, B, C); only A and B have results → C removed
    let msgs = vec![
        Message::user("go"),
        assistant_with_content(vec![
            make_tool_call("A", "tool_a"),
            make_tool_call("B", "tool_b"),
            make_tool_call("C", "tool_c"),
        ]),
        user_with_content(vec![
            make_tool_result("A", "res_a"),
            make_tool_result("B", "res_b"),
        ]),
    ];
    let mut issues = Vec::new();
    let result = fix_tool_call_integrity(msgs, &mut issues);
    assert_eq!(
        result.len(),
        3,
        "all three messages kept (C removed from assistant content)"
    );
    match &result[1] {
        Message::Assistant(assistant) => {
            let content = &assistant.content;
            assert_eq!(content.len(), 2, "A and B calls kept, C removed");
        }
        _ => panic!("expected assistant message"),
    }
    assert!(!issues.is_empty());
}

// ================================================================
// Pass 3 — merge_consecutive_same_role
// ================================================================

/// Consecutive same-role plain-text messages are merged; alternating roles
/// are left alone.
#[test]
fn merge_consecutive_same_role_merges_same_role_runs() {
    // -- Case 1: consecutive user messages are merged
    let msgs = vec![
        Message::user("first"),
        Message::user("second"),
        Message::assistant("reply"),
    ];
    let mut issues = Vec::new();
    let result = merge_consecutive_same_role(msgs, &mut issues);
    assert_eq!(result.len(), 2);
    assert!(matches!(&result[0], Message::User { .. }));
    assert!(matches!(&result[1], Message::Assistant(_)));
    match &result[0] {
        Message::User { content } => assert_eq!(content.len(), 2),
        _ => panic!("expected user message"),
    }
    assert!(!issues.is_empty());

    // -- Case 2: consecutive assistant messages are merged
    let msgs = vec![
        Message::user("go"),
        Message::assistant("part1"),
        Message::assistant("part2"),
    ];
    let mut issues = Vec::new();
    let result = merge_consecutive_same_role(msgs, &mut issues);
    assert_eq!(result.len(), 2);
    assert!(matches!(&result[0], Message::User { .. }));
    match &result[1] {
        Message::Assistant(assistant) => assert_eq!(assistant.content.len(), 2),
        _ => panic!("expected assistant message"),
    }
    assert!(!issues.is_empty());

    // -- Case 3: alternating roles are not merged
    let msgs = vec![
        Message::user("a"),
        Message::assistant("b"),
        Message::user("c"),
        Message::assistant("d"),
    ];
    let expected = msgs.clone();
    let mut issues = Vec::new();
    let result = merge_consecutive_same_role(msgs, &mut issues);
    assert_msgs_eq(&result, &expected);
    assert!(issues.is_empty());
}

/// A User message carrying a ToolResult is never merged with a neighbouring
/// User message.
#[test]
fn merge_consecutive_same_role_skips_tool_result_messages() {
    // -- Case 1: User(ToolResult) followed by User(Text) must NOT be merged:
    // OpenAI-compatible providers discard Text when ToolResult is present.
    let assistant = assistant_with_content(vec![make_tool_call("T1", "tool_one")]);
    let tool_result_msg = user_with_content(vec![make_tool_result("T1", "result")]);
    let text_msg = Message::user("follow-up question");
    let msgs = vec![assistant, tool_result_msg, text_msg];
    let mut issues = Vec::new();
    let result = merge_consecutive_same_role(msgs, &mut issues);
    assert_eq!(result.len(), 3, "messages must remain separate");
    assert!(issues.is_empty(), "no merge should be recorded");

    // -- Case 2: User(ToolResult) followed by User(ToolResult) must NOT be merged
    let assistant = assistant_with_content(vec![
        make_tool_call("A1", "tool_a"),
        make_tool_call("B1", "tool_b"),
    ]);
    let result_a = user_with_content(vec![make_tool_result("A1", "res_a")]);
    let result_b = user_with_content(vec![make_tool_result("B1", "res_b")]);
    let msgs = vec![assistant, result_a, result_b];
    let mut issues = Vec::new();
    let result = merge_consecutive_same_role(msgs, &mut issues);
    assert_eq!(result.len(), 3, "tool result messages must remain separate");
    assert!(issues.is_empty(), "no merge should be recorded");
}

// ================================================================
// Pass 4 — trim_trailing_user (direct tests)
// ================================================================

#[test]
fn trim_trailing_user_direct() {
    // -- Case 1: a single trailing user message is trimmed
    let msgs = vec![
        Message::user("hi"),
        Message::assistant("hello"),
        Message::user("orphan"),
    ];
    let mut issues = Vec::new();
    let result = trim_trailing_user(msgs, &mut issues);
    assert_eq!(result.len(), 2);
    assert!(matches!(&result[1], Message::Assistant(_)));
    assert!(!issues.is_empty());

    // -- Case 2: multiple trailing user messages are all trimmed
    let msgs = vec![
        Message::user("hi"),
        Message::assistant("hello"),
        Message::user("orphan1"),
        Message::user("orphan2"),
    ];
    let mut issues = Vec::new();
    let result = trim_trailing_user(msgs, &mut issues);
    assert_eq!(result.len(), 2);
    assert!(matches!(&result[1], Message::Assistant(_)));
}

// ================================================================
// Full pipeline
// ================================================================

#[test]
fn pipeline_is_idempotent_and_leaves_valid_input_alone() {
    // -- Case 1: repairing a corrupted history twice is a no-op the second time
    let msgs = vec![
        Message::user("go"),
        assistant_with_content(vec![make_tool_call("X", "tool")]),
        Message::user("orphan"),
    ];
    let (once, _) = repair_messages(msgs);
    let (twice, issues2) = repair_messages(once.clone());
    assert_msgs_eq(&once, &twice);
    assert!(
        issues2.is_empty(),
        "second pass should find nothing to repair"
    );

    // -- Case 2: a well-formed conversation is unchanged and reports no issues
    let msgs = vec![
        Message::user("hello"),
        Message::assistant("hi"),
        Message::user("bye"),
        Message::assistant("farewell"),
    ];
    let expected = msgs.clone();
    let (result, issues) = repair_messages(msgs);
    assert_msgs_eq(&result, &expected);
    assert!(issues.is_empty(), "no issues for well-formed conversation");
}

#[test]
fn pipeline_complex_corruption() {
    // Violations:
    // 1. Two consecutive user messages (pass 3)
    // 2. Dangling tool call (no result) (pass 2)
    // 3. Trailing orphan user message (pass 4)
    let msgs = vec![
        Message::user("first"),
        Message::user("second"), // consecutive user
        assistant_with_content(vec![
            AssistantContent::text("thinking"),
            make_tool_call("Y", "fetch"), // dangling — no result follows
        ]),
        Message::user("orphan"), // trailing user
    ];
    let (result, issues) = repair_messages(msgs);

    // After repair:
    // - consecutive users merged → 1 user message
    // - tool call removed, text preserved → assistant has 1 item (text)
    // - trailing user trimmed
    // So: [user(merged), assistant(text only)]
    assert_eq!(result.len(), 2, "merged users + assistant with text only");
    assert!(matches!(&result[0], Message::User { .. }));
    match &result[1] {
        Message::Assistant(assistant) => {
            let content = &assistant.content;
            assert_eq!(content.len(), 1);
            assert!(matches!(content.first(), Some(AssistantContent::Text(_))));
        }
        _ => panic!("expected assistant message"),
    }
    assert!(!issues.is_empty(), "repairs should produce diagnostics");
}
