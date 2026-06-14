use super::helpers::*;
use crate::types::{
    AssistantContent, Message, Text, ToolCall, ToolFunction, ToolResult, ToolResultContent,
    UserContent,
};
use rig::one_or_many::OneOrMany;
use serde_json::json;

/// Helper: build an Assistant message containing a single ToolCall.
fn make_tool_call_message(call_id: &str, tool_name: &str) -> Message {
    Message::Assistant {
        id: None,
        content: OneOrMany::one(AssistantContent::ToolCall(ToolCall {
            id: call_id.to_string(),
            call_id: None,
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: tool_name.to_string(),
                arguments: json!({}),
            },
        })),
    }
}

/// Helper: build a User message containing a single ToolResult.
fn make_tool_result_message(call_id: &str, result_text: &str) -> Message {
    Message::User {
        content: OneOrMany::one(UserContent::ToolResult(ToolResult {
            id: call_id.to_string(),
            call_id: None,
            content: OneOrMany::one(ToolResultContent::Text(Text {
                text: result_text.to_string(),
                additional_params: None,
            })),
        })),
    }
}

#[test]
fn find_safe_split_index_adjusts_for_tool_result_at_boundary() {
    // [user, user, TC_A, TR_A, user, user]
    // target=3 (TR_A is at index 3) → should move back to 2
    let messages = vec![
        Message::user("u0"),
        Message::user("u1"),
        make_tool_call_message("A", "read"),
        make_tool_result_message("A", "ok"),
        Message::user("u2"),
        Message::user("u3"),
    ];
    assert_eq!(find_safe_split_index(&messages, 3), 2);
}

#[test]
fn find_safe_split_index_adjusts_for_consecutive_pairs() {
    // [user, TC_A, TR_A, TC_B, TR_B, user]
    // target=4 (TR_B at index 4) → walks back to 3 (between TR_A and TC_B is safe)
    let messages = vec![
        Message::user("u0"),
        make_tool_call_message("A", "read"),
        make_tool_result_message("A", "ok"),
        make_tool_call_message("B", "write"),
        make_tool_result_message("B", "ok"),
        Message::user("u1"),
    ];
    assert_eq!(find_safe_split_index(&messages, 4), 3);
}

#[test]
fn find_safe_split_index_no_adjustment_when_clean() {
    // [user, user, user, user]
    // target=2 → no tool pairs, stays at 2
    let messages = vec![
        Message::user("u0"),
        Message::user("u1"),
        Message::user("u2"),
        Message::user("u3"),
    ];
    assert_eq!(find_safe_split_index(&messages, 2), 2);
}

#[test]
fn find_safe_split_index_all_tool_pairs() {
    // [TC_A, TR_A, TC_B, TR_B]
    // target=2 → boundary between TR_A and TC_B is safe, stays at 2
    let messages = vec![
        make_tool_call_message("A", "read"),
        make_tool_result_message("A", "ok"),
        make_tool_call_message("B", "write"),
        make_tool_result_message("B", "ok"),
    ];
    assert_eq!(find_safe_split_index(&messages, 2), 2);
}

#[test]
fn estimate_tokens_approximation() {
    // "hello world" = 11 chars → serialized in a Message::user has overhead,
    // but raw "hello world" text = 11 chars → 2 tokens (11/4 = 2 truncated).
    // We test via Message::user which adds JSON overhead, so we test the
    // function's proportionality instead.
    let msg_small = Message::user("hello world");
    let tokens_small = estimate_tokens(&msg_small);
    // "hello world" is 11 chars but serialized JSON is larger; verify > 0
    assert!(
        tokens_small > 0,
        "Expected non-zero tokens for 'hello world'"
    );

    let msg_large = Message::user("x".repeat(400));
    let tokens_large = estimate_tokens(&msg_large);
    // 400 chars of content → ~100 tokens of content + JSON overhead
    assert!(
        tokens_large >= 100,
        "Expected >= 100 tokens for 400 chars, got {}",
        tokens_large
    );

    // Large message should have significantly more tokens
    assert!(
        tokens_large > tokens_small,
        "Larger message should have more tokens"
    );
}
