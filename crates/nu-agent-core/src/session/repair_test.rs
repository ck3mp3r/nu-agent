use super::repair::{
    fix_tool_call_integrity, inject_missing_tool_results, merge_consecutive_same_role,
    remove_empty_messages, repair_messages, trim_trailing_user,
};
use crate::types::{AssistantContent, Message, ToolCall, ToolFunction, ToolResult, UserContent};
use rig::one_or_many::OneOrMany;

// ================================================================
// Helpers
// ================================================================

/// Compare two message slices via their serialized JSON form.
/// `Text::additional_params` is flattened by serde; a round-trip may turn `None`
/// into `Some(Object {})`, so we normalize through JSON before comparing.
fn assert_msgs_eq(left: &[Message], right: &[Message]) {
    assert_eq!(left.len(), right.len(), "message count mismatch");
    for (i, (l, r)) in left.iter().zip(right.iter()).enumerate() {
        assert_eq!(
            serde_json::to_value(l).expect("serialize left"),
            serde_json::to_value(r).expect("serialize right"),
            "message {i} mismatch",
        );
    }
}

fn make_tool_call(id: &str, name: &str) -> AssistantContent {
    AssistantContent::ToolCall(ToolCall::new(
        id.to_string(),
        ToolFunction::new(name.to_string(), serde_json::json!({})),
    ))
}

fn make_tool_result(id: &str, output: &str) -> UserContent {
    UserContent::ToolResult(ToolResult {
        id: id.to_string(),
        call_id: None,
        content: OneOrMany::one(crate::types::ToolResultContent::text(output)),
    })
}

fn assistant_with_content(items: Vec<AssistantContent>) -> Message {
    Message::Assistant {
        id: None,
        content: OneOrMany::many(items).expect("non-empty assistant content"),
    }
}

fn user_with_content(items: Vec<UserContent>) -> Message {
    Message::User {
        content: OneOrMany::many(items).expect("non-empty user content"),
    }
}

// ================================================================
// Pass 1 — remove_empty_messages
// ================================================================

#[test]
fn remove_empty_user_message() {
    // OneOrMany<T> cannot hold zero items, so "structurally empty" is
    // impossible at the type level. The only user-empty case is all-empty text
    // with no ToolResults.
    let msg = user_with_content(vec![UserContent::Text(crate::types::Text::new(""))]);
    let mut issues = Vec::new();
    let result = remove_empty_messages(vec![msg], &mut issues);
    assert!(
        result.is_empty(),
        "user message with empty text must be removed"
    );
    assert!(!issues.is_empty(), "should emit a diagnostic");
}

#[test]
fn remove_empty_assistant_message() {
    // Same reasoning as above — all-whitespace text, no ToolCalls.
    let msg = assistant_with_content(vec![AssistantContent::text("   ")]);
    let mut issues = Vec::new();
    let result = remove_empty_messages(vec![msg], &mut issues);
    assert!(
        result.is_empty(),
        "assistant message with whitespace-only text must be removed"
    );
    assert!(!issues.is_empty());
}

#[test]
fn remove_empty_text_assistant_message() {
    // AssistantContent::Text("") counts as empty
    let msg = assistant_with_content(vec![AssistantContent::text("")]);
    let mut issues = Vec::new();
    let result = remove_empty_messages(vec![msg], &mut issues);
    assert!(
        result.is_empty(),
        "assistant message with empty text must be removed"
    );
    assert!(!issues.is_empty());
}

#[test]
fn preserve_non_empty_messages() {
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

#[test]
fn remove_dangling_tool_call_no_result() {
    // Assistant has ToolCall{id:"X"}, no following ToolResult{id:"X"} → message removed
    let msgs = vec![
        Message::user("hi"),
        assistant_with_content(vec![make_tool_call("X", "do_thing")]),
    ];
    let mut issues = Vec::new();
    let result = fix_tool_call_integrity(msgs, &mut issues);
    // The orphaned assistant message should be removed; only user remains
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], Message::User { .. }));
    assert!(!issues.is_empty());
}

#[test]
fn preserve_tool_call_with_matching_result() {
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

#[test]
fn remove_orphaned_tool_result_no_call() {
    // User has ToolResult{id:"Z"}, but no preceding ToolCall{id:"Z"}
    let msgs = vec![
        Message::user("hi"),
        user_with_content(vec![make_tool_result("Z", "some result")]),
    ];
    let mut issues = Vec::new();
    let result = fix_tool_call_integrity(msgs, &mut issues);
    // Orphaned ToolResult message removed; only the first user message remains
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], Message::User { .. }));
    assert!(!issues.is_empty());
}

#[test]
fn tool_call_with_text_preserves_text() {
    // Assistant has [Text("hello"), ToolCall{id:"X"}]; no result → ToolCall removed, Text preserved
    let msgs = vec![
        Message::user("hi"),
        assistant_with_content(vec![
            AssistantContent::text("hello"),
            make_tool_call("X", "do_thing"),
        ]),
    ];
    let mut issues = Vec::new();
    let result = fix_tool_call_integrity(msgs, &mut issues);
    // Assistant message kept with only Text content
    assert_eq!(result.len(), 2);
    match &result[1] {
        Message::Assistant { content, .. } => {
            assert_eq!(content.len(), 1);
            assert!(matches!(content.first_ref(), AssistantContent::Text(_)));
        }
        _ => panic!("expected assistant message"),
    }
    assert!(!issues.is_empty());
}

#[test]
fn multiple_tool_calls_partial_match() {
    // 3 tool calls (A, B, C); only A and B have results → C removed
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
        Message::Assistant { content, .. } => {
            assert_eq!(content.len(), 2, "A and B calls kept, C removed");
        }
        _ => panic!("expected assistant message"),
    }
    assert!(!issues.is_empty());
}

// ================================================================
// Pass 3 — merge_consecutive_same_role
// ================================================================

#[test]
fn merge_consecutive_user_messages() {
    let msgs = vec![
        Message::user("first"),
        Message::user("second"),
        Message::assistant("reply"),
    ];
    let mut issues = Vec::new();
    let result = merge_consecutive_same_role(msgs, &mut issues);
    assert_eq!(result.len(), 2);
    assert!(matches!(&result[0], Message::User { .. }));
    assert!(matches!(&result[1], Message::Assistant { .. }));
    // Merged user should have 2 content items
    match &result[0] {
        Message::User { content } => assert_eq!(content.len(), 2),
        _ => panic!("expected user message"),
    }
    assert!(!issues.is_empty());
}

#[test]
fn merge_consecutive_assistant_messages() {
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
        Message::Assistant { content, .. } => assert_eq!(content.len(), 2),
        _ => panic!("expected assistant message"),
    }
    assert!(!issues.is_empty());
}

#[test]
fn no_merge_for_alternating() {
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

// ================================================================
// Pass 3 — ToolResult guard (merge_consecutive_same_role)
// ================================================================

#[test]
fn user_tool_result_and_text_not_merged() {
    // User(ToolResult) followed by User(Text) must NOT be merged:
    // OpenAI-compatible providers discard Text when ToolResult is present.
    let assistant = assistant_with_content(vec![make_tool_call("T1", "tool_one")]);
    let tool_result_msg = user_with_content(vec![make_tool_result("T1", "result")]);
    let text_msg = Message::user("follow-up question");
    let msgs = vec![assistant, tool_result_msg, text_msg];
    let mut issues = Vec::new();
    let result = merge_consecutive_same_role(msgs, &mut issues);
    assert_eq!(result.len(), 3, "messages must remain separate");
    assert!(issues.is_empty(), "no merge should be recorded");
}

#[test]
fn plain_text_users_still_merged() {
    // User(Text) followed by User(Text) IS still merged — existing behaviour preserved.
    let msgs = vec![
        Message::user("first"),
        Message::user("second"),
        Message::assistant("reply"),
    ];
    let mut issues = Vec::new();
    let result = merge_consecutive_same_role(msgs, &mut issues);
    assert_eq!(
        result.len(),
        2,
        "two text-only user messages must be merged"
    );
    match &result[0] {
        Message::User { content } => assert_eq!(content.len(), 2),
        _ => panic!("expected user message"),
    }
    assert!(!issues.is_empty(), "merge must emit a diagnostic");
}

#[test]
fn tool_result_followed_by_tool_result_not_merged() {
    // User(ToolResult) followed by User(ToolResult) must NOT be merged.
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
    let msgs = vec![
        Message::user("hi"),
        Message::assistant("hello"),
        Message::user("orphan"),
    ];
    let mut issues = Vec::new();
    let result = trim_trailing_user(msgs, &mut issues);
    assert_eq!(result.len(), 2);
    assert!(matches!(&result[1], Message::Assistant { .. }));
    assert!(!issues.is_empty());
}

#[test]
fn trim_multiple_trailing_users_direct() {
    let msgs = vec![
        Message::user("hi"),
        Message::assistant("hello"),
        Message::user("orphan1"),
        Message::user("orphan2"),
    ];
    let mut issues = Vec::new();
    let result = trim_trailing_user(msgs, &mut issues);
    assert_eq!(result.len(), 2);
    assert!(matches!(&result[1], Message::Assistant { .. }));
}

// ================================================================
// Full pipeline
// ================================================================

#[test]
fn pipeline_is_idempotent() {
    // Corrupted: dangling tool call, trailing user
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
}

#[test]
fn pipeline_valid_input_no_changes_no_issues() {
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
        Message::Assistant { content, .. } => {
            assert_eq!(content.len(), 1);
            assert!(matches!(content.first_ref(), AssistantContent::Text(_)));
        }
        _ => panic!("expected assistant message"),
    }
    assert!(!issues.is_empty(), "repairs should produce diagnostics");
}

// ================================================================
// New failing tests — TDD RED phase
// ================================================================

/// Test 1 — `trim_trailing_user` must NOT pop a User message that contains
/// a ToolResult.  Before the fix, the trailing User(ToolResult) is popped,
/// leaving the Assistant(ToolCall) dangling.
#[test]
fn trim_trailing_user_preserves_tool_result() {
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
}

/// Test 2 — `fix_tool_call_integrity` must enforce adjacency.
/// A ToolCall and its ToolResult that are separated by another message are
/// non-adjacent and must be stripped.  Before the fix, the current global-ID
/// matching passes them through unchanged.
#[test]
fn fix_tool_call_integrity_strips_non_adjacent_pair() {
    let msgs = vec![
        Message::user("start"),
        assistant_with_content(vec![make_tool_call("abc", "do_thing")]),
        Message::user("in between"),
        user_with_content(vec![make_tool_result("abc", "ok")]),
    ];
    let (result, _) = repair_messages(msgs);

    let has_tool_call = result.iter().any(|msg| match msg {
        Message::Assistant { content, .. } => content
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

/// Test 3 — `merge_consecutive_same_role` must NOT merge two consecutive
/// Assistant messages when either contains a ToolCall.
/// Before the fix, they are merged unconditionally.
/// We call the pass directly so the adjacency enforcement in
/// `fix_tool_call_integrity` does not confound the assertion.
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
        .filter(|msg| matches!(msg, Message::Assistant { .. }))
        .count();
    assert_eq!(
        assistant_count, 2,
        "two ToolCall-bearing Assistant messages must NOT be merged into one"
    );
    assert!(issues.is_empty(), "no merge diagnostic should be emitted");
}

// ================================================================
// inject_missing_tool_results — TDD RED phase
// ================================================================

/// Test: history with unpaired `Assistant(ToolCall{id:"x"})` →
/// result contains `User(ToolResult{id:"x", content:"[interrupted]"})`
/// immediately after it.
#[test]
fn inject_missing_tool_results_inserts_synthetic_result_for_unpaired_call() {
    let msgs = vec![
        Message::user("go"),
        assistant_with_content(vec![make_tool_call("x", "do_thing")]),
    ];
    let result = inject_missing_tool_results(msgs);
    // Should have 3 messages: user, assistant(ToolCall), user(ToolResult)
    assert_eq!(result.len(), 3, "synthetic ToolResult must be injected");
    // The injected message is at index 2
    match &result[2] {
        Message::User { content } => {
            let has_tool_result = content.iter().any(|item| match item {
                UserContent::ToolResult(tr) => tr.id == "x",
                _ => false,
            });
            assert!(
                has_tool_result,
                "injected User message must contain ToolResult with id='x'"
            );
        }
        _ => panic!("message[2] must be a User message with ToolResult"),
    }
}

/// Test: fully paired history → result is identical to input (nothing injected).
#[test]
fn inject_missing_tool_results_leaves_paired_history_unchanged() {
    let msgs = vec![
        Message::user("prompt"),
        assistant_with_content(vec![make_tool_call("x", "do_thing")]),
        user_with_content(vec![make_tool_result("x", "ok")]),
    ];
    let expected = msgs.clone();
    let result = inject_missing_tool_results(msgs);
    assert_msgs_eq(&result, &expected);
}

/// Test: `Assistant` message with two unpaired ToolCalls (`id:"a"` and `id:"b"`) →
/// both get synthetic results, injected as a single `User` message with both
/// `ToolResult` entries (one `User` message covers all ToolCalls from one `Assistant` message).
#[test]
fn inject_missing_tool_results_groups_two_unpaired_calls_into_one_user_message() {
    let msgs = vec![
        Message::user("go"),
        assistant_with_content(vec![
            make_tool_call("a", "tool_a"),
            make_tool_call("b", "tool_b"),
        ]),
    ];
    let result = inject_missing_tool_results(msgs);
    // Should be: user, assistant(a+b), user(ToolResult{a} + ToolResult{b})
    assert_eq!(result.len(), 3, "one synthetic User message for both calls");
    match &result[2] {
        Message::User { content } => {
            let result_ids: Vec<&str> = content
                .iter()
                .filter_map(|item| match item {
                    UserContent::ToolResult(tr) => Some(tr.id.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                result_ids.contains(&"a"),
                "ToolResult for 'a' must be present"
            );
            assert!(
                result_ids.contains(&"b"),
                "ToolResult for 'b' must be present"
            );
            assert_eq!(
                result_ids.len(),
                2,
                "exactly two ToolResults in one User message"
            );
        }
        _ => panic!("message[2] must be a User message"),
    }
}

/// Test 4 — Pass ordering: `trim_trailing_user` must not leave a dangling
/// ToolCall in the output.  Currently `fix_tool_call_integrity` runs before
/// `trim_trailing_user`; after trim pops the ToolResult, there is no second
/// integrity pass and the ToolCall is left dangling.
#[test]
fn pipeline_no_dangling_tool_call_after_trim() {
    let msgs = vec![
        Message::user("q"),
        assistant_with_content(vec![make_tool_call("abc", "do_thing")]),
        user_with_content(vec![make_tool_result("abc", "ok")]),
    ];
    let (result, _) = repair_messages(msgs);

    // Walk the result; for every Assistant message with ToolCalls, the very
    // next message must be a User message containing matching ToolResults.
    for (i, msg) in result.iter().enumerate() {
        let Message::Assistant { content, .. } = msg else {
            continue;
        };
        let call_ids: Vec<&str> = content
            .iter()
            .filter_map(|item| match item {
                AssistantContent::ToolCall(tc) => Some(tc.id.as_str()),
                _ => None,
            })
            .collect();
        if call_ids.is_empty() {
            continue;
        }
        let next = result.get(i + 1);
        let next_result_ids: Vec<&str> = match next {
            Some(Message::User { content }) => content
                .iter()
                .filter_map(|item| match item {
                    UserContent::ToolResult(tr) => Some(tr.id.as_str()),
                    _ => None,
                })
                .collect(),
            _ => vec![],
        };
        for id in &call_ids {
            assert!(
                next_result_ids.contains(id),
                "ToolCall id={id} has no adjacent ToolResult in message[{}]; result length={}",
                i + 1,
                result.len()
            );
        }
    }
}
