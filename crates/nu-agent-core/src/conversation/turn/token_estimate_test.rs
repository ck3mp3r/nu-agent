use super::estimate_token_count;
use crate::types::{Message, Text, ToolResult, ToolResultContent, UserContent};
use rig::one_or_many::OneOrMany;

#[test]
fn estimate_token_count_empty_returns_zero() {
    assert_eq!(estimate_token_count(&[]), 0);
}

#[test]
fn estimate_token_count_per_message_overhead() {
    // 1 message with 0 content chars:
    // (0 / 4) + (1 * 4) = 0 + 4 = 4
    let msg = Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: String::new(),
            additional_params: None,
        })),
    };
    assert_eq!(estimate_token_count(&[msg]), 4);
}

#[test]
fn estimate_token_count_plain_text() {
    // "hello world" = 11 chars
    // 11 / 4 = 2 (integer division), + 4 overhead = 6
    let msg = Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "hello world".to_string(),
            additional_params: None,
        })),
    };
    assert_eq!(estimate_token_count(&[msg]), 6);
}

#[test]
fn estimate_token_count_tool_result() {
    // tool_result content = "abcdefghijkl" = 12 chars
    // 12 / 4 = 3, + 4 overhead = 7
    let msg = Message::User {
        content: OneOrMany::one(UserContent::ToolResult(ToolResult {
            id: "tc1".to_string(),
            call_id: None,
            content: OneOrMany::one(ToolResultContent::Text(Text {
                text: "abcdefghijkl".to_string(),
                additional_params: None,
            })),
        })),
    };
    assert_eq!(estimate_token_count(&[msg]), 7);
}

/// Conservative note: for `"for i in range(100): print(i)"` (29 chars),
/// tiktoken gives ~10 tokens (code ≈ 3 chars/token).
/// Our heuristic gives `29/4 + 4 = 7 + 4 = 11`.
/// For code, tiktoken gives ~10 but chars/4 gives ~7, so we **under-count** vs tiktoken.
/// The under-count is even larger for dense JSON or identifiers.
/// This is why we use a 60% threshold rather than 80%.
#[test]
fn estimate_token_count_conservative_note() {
    // This test documents the conservative nature of the estimator.
    // "for i in range(100): print(i)" = 29 chars
    // Our estimate: 29/4 + 4 = 7 + 4 = 11
    // tiktoken would give ~10 (code is denser than 4 chars/token), so we under-count.
    let msg = Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "for i in range(100): print(i)".to_string(),
            additional_params: None,
        })),
    };
    let estimated = estimate_token_count(&[msg]);
    assert_eq!(estimated, 11);
}
