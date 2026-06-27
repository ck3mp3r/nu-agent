use super::repair::{
    fix_tool_call_integrity, merge_consecutive_same_role, remove_empty_messages, repair_messages,
    trim_trailing_user,
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
    let msg = user_with_content(vec![UserContent::Text(
        crate::types::Text::new(""),
    )]);
    let mut issues = Vec::new();
    let result = remove_empty_messages(vec![msg], &mut issues);
    assert!(result.is_empty(), "user message with empty text must be removed");
    assert!(!issues.is_empty(), "should emit a diagnostic");
}

#[test]
fn remove_empty_assistant_message() {
    // Same reasoning as above — all-whitespace text, no ToolCalls.
    let msg = assistant_with_content(vec![AssistantContent::text("   ")]);
    let mut issues = Vec::new();
    let result = remove_empty_messages(vec![msg], &mut issues);
    assert!(result.is_empty(), "assistant message with whitespace-only text must be removed");
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
    assert_eq!(result.len(), 3, "all three messages kept (C removed from assistant content)");
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
    assert!(issues2.is_empty(), "second pass should find nothing to repair");
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
        Message::user("second"),           // consecutive user
        assistant_with_content(vec![
            AssistantContent::text("thinking"),
            make_tool_call("Y", "fetch"),  // dangling — no result follows
        ]),
        Message::user("orphan"),           // trailing user
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
