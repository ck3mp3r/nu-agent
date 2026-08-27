use super::repair::{
    fix_empty_tool_results, fix_null_tool_arguments, fix_tool_call_integrity,
    inject_assistant_after_dangling_tool_results, inject_missing_tool_results,
    merge_consecutive_same_role, remove_empty_messages, repair_messages, trim_trailing_user,
};
use super::store_test::assert_msgs_eq;
use crate::types::{
    AssistantContent, Message, ToolCall, ToolCallId, ToolFunction, ToolResult, UserContent,
};

fn make_tool_call(id: &str, name: &str) -> AssistantContent {
    AssistantContent::ToolCall(ToolCall::new(
        ToolCallId::new_or_mint(id),
        ToolFunction::new(name.to_string(), serde_json::json!({})),
    ))
}

fn make_tool_result(id: &str, output: &str) -> UserContent {
    UserContent::ToolResult(ToolResult {
        call: ToolCallId::new_or_mint(id),
        provider: None,
        name: "read_file".into(),
        content: vec![crate::types::ToolResultContent::text(output)],
    })
}

fn assistant_with_content(items: Vec<AssistantContent>) -> Message {
    Message::Assistant {
        id: None,
        content: items,
    }
}

fn user_with_content(items: Vec<UserContent>) -> Message {
    Message::User { content: items }
}

// ================================================================
// Pass 1 — remove_empty_messages
// ================================================================

#[test]
fn remove_empty_user_message() {
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
            assert!(matches!(content.first(), Some(AssistantContent::Text(_))));
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
            assert!(matches!(content.first(), Some(AssistantContent::Text(_))));
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
// inject_assistant_after_dangling_tool_results
// ================================================================

#[test]
fn inject_assistant_after_dangling_tool_results_heals_corrupt_session() {
    // Input: user(prompt), asst([tool_call("tc1")]), user([tool_result("tc1")]), user("continue")
    // The user([tool_result]) followed by user("continue") is the corrupt pattern.
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
    // The injected message is at index 3
    match &result[3] {
        Message::Assistant { content, .. } => {
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
}

#[test]
fn inject_assistant_after_dangling_tool_results_noop_when_valid() {
    let msgs = vec![Message::user("p"), Message::assistant("ok")];
    let expected = msgs.clone();
    let mut issues = Vec::new();
    let result = inject_assistant_after_dangling_tool_results(msgs, &mut issues);
    assert_msgs_eq(&result, &expected);
    assert!(issues.is_empty(), "no issues for valid input");
}

#[test]
fn inject_assistant_after_dangling_tool_results_noop_after_gap1a() {
    // Already healed by Gap 1A: asst closes the block after tool results
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
}

#[test]
fn inject_assistant_after_dangling_tool_results_idempotent() {
    // First pass: corrupt input → healed
    let msgs = vec![
        Message::user("prompt"),
        assistant_with_content(vec![make_tool_call("tc1", "do_thing")]),
        user_with_content(vec![make_tool_result("tc1", "output")]),
        Message::user("continue"),
    ];
    let mut issues1 = Vec::new();
    let healed = inject_assistant_after_dangling_tool_results(msgs, &mut issues1);
    assert!(!issues1.is_empty(), "first pass must emit a diagnostic");

    // Second pass: healed → identical, no new issues
    let healed_clone = healed.clone();
    let mut issues2 = Vec::new();
    let healed_again = inject_assistant_after_dangling_tool_results(healed, &mut issues2);
    assert_msgs_eq(&healed_again, &healed_clone);
    assert!(
        issues2.is_empty(),
        "second pass must emit no issues (idempotent); issues: {issues2:?}"
    );
}

#[test]
fn repair_messages_heals_session_jsonl_pattern() {
    // Simulate the real broken session pattern:
    //   user(prompt)
    //   asst([tool_call(tc_read1), tool_call(tc_read2), tool_call(tc_ls)])
    //   user([tool_result(tc_read1), tool_result(tc_read2), tool_result(tc_ls)])  ← pure ToolResult
    //   user("continue")  ← next is User → inject assistant between them
    //
    // After inject_assistant_after_dangling_tool_results, there are 5 messages.
    // Then trim_trailing_user removes the trailing user("continue") (text-only, no ToolResult),
    // yielding 4 messages. The injected assistant is at index 3 (the final message).
    let msgs = vec![
        Message::user("prompt"),
        assistant_with_content(vec![
            make_tool_call("tc_read1", "read"),
            make_tool_call("tc_read2", "read"),
            make_tool_call("tc_ls", "ls"),
        ]),
        user_with_content(vec![
            make_tool_result("tc_read1", "content1"),
            make_tool_result("tc_read2", "content2"),
            make_tool_result("tc_ls", "skills"),
        ]),
        Message::user("continue"),
    ];
    let (result, issues) = repair_messages(msgs);

    // inject adds asst("[interrupted]") before user("continue") → 5 messages,
    // then trim_trailing_user removes the trailing user("continue") → 4 messages.
    assert_eq!(
        result.len(),
        4,
        "expected 4 messages after repair; got: {result:?}"
    );
    // result[3] must be the injected assistant (last message after trim)
    match &result[3] {
        Message::Assistant { content, .. } => {
            let text = content.iter().find_map(|c| match c {
                crate::types::AssistantContent::Text(t) => Some(t.text.as_str()),
                _ => None,
            });
            assert!(
                text.is_some_and(|t| t.contains("[interrupted")),
                "result[3] must be the injected assistant message; text: {text:?}"
            );
        }
        _ => panic!(
            "result[3] must be an Assistant message; got: {:?}",
            result[3]
        ),
    }
    assert!(
        !issues.is_empty(),
        "repair_messages must emit diagnostics for the corrupt input"
    );
}

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
                UserContent::ToolResult(tr) => tr.call.as_str() == "x",
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
                    UserContent::ToolResult(tr) => Some(tr.call.as_str()),
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
                    UserContent::ToolResult(tr) => Some(tr.call.as_str()),
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

// ================================================================
// Gap 6 — fix_empty_tool_results tests
// ================================================================

/// Test 1 — `fix_empty_tool_results` replaces empty-text ToolResult content
/// with `"(empty result)"` and logs a diagnostic.
#[test]
fn fix_empty_tool_results_replaces_empty_text() {
    let msgs = vec![user_with_content(vec![make_tool_result("tc1", "")])];
    let mut issues = Vec::new();
    let result = fix_empty_tool_results(msgs, &mut issues);

    // Content must be replaced with "(empty result)"
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
        !issues.is_empty(),
        "must emit a diagnostic for replaced content"
    );
    assert!(
        issues.iter().any(|i| i.contains("tc1")),
        "diagnostic must reference tool result id=tc1; issues: {issues:?}"
    );
}

#[test]
fn fix_empty_tool_results_replaces_whitespace_only() {
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

/// Test 2 — Non-empty content is not modified and no issues are logged.
#[test]
fn fix_empty_tool_results_noop_when_content_present() {
    let msgs = vec![user_with_content(vec![make_tool_result(
        "tc2",
        "actual output",
    )])];
    let original = msgs.clone();
    let mut issues = Vec::new();
    let result = fix_empty_tool_results(msgs, &mut issues);

    assert_msgs_eq(&result, &original);
    assert!(issues.is_empty(), "no issues for non-empty tool result");
}

/// Test 3 — Assistant and plain User messages are not modified.
#[test]
fn fix_empty_tool_results_noop_on_non_user_messages() {
    let msgs = vec![Message::assistant("text"), Message::user("plain user text")];
    let original = msgs.clone();
    let mut issues = Vec::new();
    let result = fix_empty_tool_results(msgs, &mut issues);

    assert_msgs_eq(&result, &original);
    assert!(issues.is_empty());
}

/// Test 4 — Full `repair_messages` pipeline includes empty ToolResult fix.
#[test]
fn repair_messages_pipeline_includes_empty_tool_result_fix() {
    let msgs = vec![
        Message::user("go"),
        assistant_with_content(vec![make_tool_call("tcX", "tool_x")]),
        user_with_content(vec![make_tool_result("tcX", "")]),
        Message::assistant("done"),
    ];
    let (result, issues) = repair_messages(msgs);

    // The empty ToolResult must have been replaced
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

// ================================================================
// fix_null_tool_arguments tests
// ================================================================

fn make_null_args_tool_call(id: &str, name: &str) -> AssistantContent {
    AssistantContent::ToolCall(ToolCall::new(
        ToolCallId::new_or_mint(id),
        ToolFunction::new(name.to_string(), serde_json::Value::Null),
    ))
}

#[test]
fn fix_null_tool_arguments_replaces_null_args() {
    let msgs = vec![
        Message::user("prompt"),
        assistant_with_content(vec![make_null_args_tool_call(
            "tc1",
            "tmux__send_and_capture",
        )]),
        user_with_content(vec![make_tool_result("tc1", "Tool not available")]),
    ];
    let mut issues = Vec::new();
    let result = fix_null_tool_arguments(msgs, &mut issues);

    // The ToolCall arguments must be replaced with {}
    match &result[1] {
        Message::Assistant { content, .. } => {
            let tc = content.iter().find_map(|c| match c {
                AssistantContent::ToolCall(tc) => Some(tc),
                _ => None,
            });
            let tc = tc.expect("ToolCall must still be present");
            assert_eq!(
                tc.function.arguments,
                serde_json::json!({}),
                "null arguments must be replaced with {{}}"
            );
        }
        _ => panic!("expected Assistant message at index 1"),
    }
    assert!(
        issues
            .iter()
            .any(|i| i.contains("replaced null tool call arguments")),
        "must emit diagnostic; issues: {issues:?}"
    );
}

#[test]
fn fix_null_tool_arguments_noop_when_valid() {
    let msgs = vec![
        Message::user("prompt"),
        assistant_with_content(vec![make_tool_call("tc1", "nu__shell")]),
        user_with_content(vec![make_tool_result("tc1", "file1")]),
    ];
    let original = msgs.clone();
    let mut issues = Vec::new();
    let result = fix_null_tool_arguments(msgs, &mut issues);
    assert_msgs_eq(&result, &original);
    assert!(issues.is_empty(), "no issues for valid args");
}

#[test]
fn repair_messages_heals_null_args_end_to_end() {
    let msgs = vec![
        Message::user("prompt"),
        assistant_with_content(vec![make_null_args_tool_call(
            "tc1",
            "tmux__send_and_capture",
        )]),
        user_with_content(vec![make_tool_result("tc1", "Tool not available")]),
        Message::assistant("ok"),
    ];
    let (result, issues) = repair_messages(msgs);

    // Find the ToolCall and verify its arguments are {}
    let tc_args = result.iter().find_map(|msg| match msg {
        Message::Assistant { content, .. } => content.iter().find_map(|c| match c {
            AssistantContent::ToolCall(tc) => Some(tc.function.arguments.clone()),
            _ => None,
        }),
        _ => None,
    });
    assert_eq!(
        tc_args,
        Some(serde_json::json!({})),
        "repair_messages must heal null ToolCall arguments; result: {result:?}"
    );
    assert!(
        issues
            .iter()
            .any(|i| i.contains("replaced null tool call arguments")),
        "must emit null-args diagnostic; issues: {issues:?}"
    );
}

// ================================================================
// inject_missing_tool_results — call_id propagation
// ================================================================

/// Test: synthetic `ToolResult` produced for an unpaired `ToolCall` with
/// `call_id: Some("call_abc123")` must carry that `call_id` through.
#[test]
fn inject_missing_tool_results_preserves_call_id() {
    let tc = AssistantContent::ToolCall(ToolCall {
        id: ToolCallId::new_or_mint("id_x"),
        provider: Some(crate::types::ProviderCallId {
            call_id: "call_abc123".to_string(),
            item_id: None,
        }),
        signature: None,
        additional_params: None,
        function: ToolFunction::new("do_thing".to_string(), serde_json::json!({})),
    });
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
                tr.provider,
                Some(crate::types::ProviderCallId {
                    call_id: "call_abc123".to_string(),
                    item_id: None,
                }),
                "synthetic ToolResult must carry call_id from its ToolCall"
            );
        }
        _ => panic!("message[2] must be a User message"),
    }
}

/// Test: synthetic `ToolResult` produced for an unpaired `ToolCall` with
/// `call_id: None` must also have `call_id: None`.
#[test]
fn inject_missing_tool_results_preserves_none_call_id() {
    let tc = AssistantContent::ToolCall(ToolCall {
        id: ToolCallId::new_or_mint("id_y"),
        provider: None,
        signature: None,
        additional_params: None,
        function: ToolFunction::new("do_other".to_string(), serde_json::json!({})),
    });
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
                tr.provider, None,
                "synthetic ToolResult must have call_id: None when ToolCall has call_id: None"
            );
        }
        _ => panic!("message[2] must be a User message"),
    }
}

// ================================================================
// CompactingMemory artifact shape — RED tests
// ================================================================

/// The compaction summary artifact is spliced as a leading `Message::User`
/// whose `Text` block carries `additional_params` marking it as a compaction
/// summary via `COMPACTION_SUMMARY_KEY`.
fn summary_artifact(summary: &str) -> Message {
    Message::User {
        content: vec![UserContent::Text(crate::types::Text {
            text: format!("What we did thus far:\n\n{summary}"),
            additional_params: crate::types::AdditionalParams::from_entries([(
                crate::conversation::compaction::compactor::COMPACTION_SUMMARY_KEY,
                serde_json::json!(true),
            )]),
        })],
    }
}

/// True if any `UserContent` block in `content` is marked as a compaction
/// summary via `COMPACTION_SUMMARY_KEY` in its `additional_params`.
fn is_summary_content(content: &[UserContent]) -> bool {
    let key = crate::conversation::compaction::compactor::COMPACTION_SUMMARY_KEY;
    content.iter().any(|c| match c {
        UserContent::Text(t) => t
            .additional_params
            .as_ref()
            .and_then(|p| p.get(key))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        _ => false,
    })
}

/// `trim_trailing_user` must NOT remove a leading `Message::User` artifact
/// when it is followed by other messages. The pass only pops from the end, so
/// a leading artifact must survive even when a genuine trailing user is trimmed.
#[test]
fn trim_trailing_user_preserves_leading_artifact() {
    // -- Setup & Fixtures
    let msgs = vec![
        summary_artifact("compacted summary"),
        Message::assistant("kept reply"),
        Message::user("trailing turn"),
    ];
    let mut issues = Vec::new();

    // -- Exec
    let result = trim_trailing_user(msgs, &mut issues);

    // -- Check
    // The trailing text-only user is trimmed, but the leading artifact survives.
    assert_eq!(result.len(), 2, "only the trailing user is trimmed");
    match &result[0] {
        Message::User { content } => assert!(
            is_summary_content(content),
            "leading artifact must survive trim_trailing_user"
        ),
        _ => panic!("expected user message"),
    }
    assert!(
        !issues.is_empty(),
        "trailing user trim must emit a diagnostic"
    );
}

/// `merge_consecutive_same_role` still merges consecutive plain-text `User`
/// messages (current behavior preserved).
#[test]
fn merge_consecutive_same_role_merges_plain_text_users() {
    // -- Setup & Fixtures
    let msgs = vec![
        Message::user("first"),
        Message::user("second"),
        Message::assistant("reply"),
    ];
    let mut issues = Vec::new();

    // -- Exec
    let result = merge_consecutive_same_role(msgs, &mut issues);

    // -- Check
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

/// `merge_consecutive_same_role` must NOT merge a summary `Message::User`
/// (carrying `additional_params` with `COMPACTION_SUMMARY_KEY`) with the next
/// `Message::User`. The summary is conversation context, not a user turn —
/// folding it into the next user message would lose it.
#[test]
fn merge_consecutive_same_role_does_not_merge_summary_with_next_user() {
    // -- Setup & Fixtures
    let msgs = vec![
        summary_artifact("compacted summary"),
        Message::user("kept turn"),
        Message::assistant("reply"),
    ];
    let mut issues = Vec::new();

    // -- Exec
    let result = merge_consecutive_same_role(msgs, &mut issues);

    // -- Check
    assert_eq!(
        result.len(),
        3,
        "summary artifact must NOT be merged with the next user message"
    );
    match &result[0] {
        Message::User { content } => {
            assert_eq!(content.len(), 1, "summary must remain its own message");
            assert!(
                is_summary_content(content),
                "first message must still be the summary artifact"
            );
        }
        _ => panic!("expected user message"),
    }
    assert!(
        issues.is_empty(),
        "no merge should be recorded for the summary artifact"
    );
}

/// `fix_tool_call_integrity` still removes dangling `ToolCall` / orphaned
/// `ToolResult` pairs after the CompactingMemory artifact shape.
#[test]
fn fix_tool_call_integrity_still_removes_dangling_tool_call() {
    // -- Setup & Fixtures
    let msgs = vec![
        summary_artifact("compacted summary"),
        Message::user("kept turn"),
        assistant_with_content(vec![make_tool_call("X", "do_thing")]),
    ];
    let mut issues = Vec::new();

    // -- Exec
    let result = fix_tool_call_integrity(msgs, &mut issues);

    // -- Check
    let has_tool_call = result.iter().any(|msg| match msg {
        Message::Assistant { content, .. } => content
            .iter()
            .any(|i| matches!(i, AssistantContent::ToolCall(_))),
        _ => false,
    });
    assert!(
        !has_tool_call,
        "dangling ToolCall must be removed by fix_tool_call_integrity"
    );
    assert!(!issues.is_empty(), "integrity pass must emit a diagnostic");
}
