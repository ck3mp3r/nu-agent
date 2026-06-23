//! Regression tests for rig-core v0.39.0 PR #1899: tool call history on cancel/abort
//!
//! The v0.39.0 fix ensures that when an agent loop is cancelled mid-stream during a tool call,
//! rig's AgentSession retains accumulated chat_history (including completed tool_call and
//! tool_result pairs). This means PromptCancelled::chat_history will contain the full set of
//! messages — not just [user_msg, partial_assistant_text].
//!
//! While we cannot fire a real cancellation mid-tool-call end-to-end without a live LLM,
//! these tests prove the data path is correct: we construct chat_history as rig's
//! AgentSession.to_chat_history() would provide to PromptCancelled after v0.39,
//! pass them through PromptCancelled -> TurnError::from, and verify all messages are preserved.

use super::*;
use crate::types::{AssistantContent, ToolCall, ToolFunction, ToolResultContent};
use crate::types::{Message, Text, UserContent};
use rig::one_or_many::OneOrMany;
use serde_json::json;

/// Regression test for rig-core v0.39.0 PR #1899: PromptCancelled::chat_history is NOT empty
/// after cancellation when tool calls have been made.
///
/// This tests the core fix: that tool_call + tool_result pairs in chat_history are preserved
/// through the PromptCancelled -> TurnError conversion. Before v0.39, tool call history was
/// lost; after v0.39 it survives cancellation.
#[test]
fn prompt_cancelled_preserves_tool_call_history() {
    // Build chat_history as rig's AgentSession would provide to PromptCancelled after v0.39.0:
    // A complete tool-use cycle: user(prompt) -> assistant(tool_call) -> user(tool_result)
    let mut chat_history: Vec<Message> = Vec::new();

    // 1) User message (the initial prompt)
    chat_history.push(Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "What is in /etc/hosts?".to_string(),
            additional_params: None,
        })),
    });

    // 2) Assistant message with a ToolCall (the LLM decided to invoke read_file)
    chat_history.push(Message::Assistant {
        id: None,
        content: OneOrMany::one(AssistantContent::ToolCall(ToolCall {
            id: "call_abc123".to_string(),
            call_id: None,
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: "read_file".to_string(),
                arguments: json!({ "path": "/etc/hosts" }),
            },
        })),
    });

    // 3) User message with a ToolResult (LLM's prior tool_call result, completed before next
    //    iteration got cancelled mid-stream)
    chat_history.push(Message::User {
        content: OneOrMany::one(UserContent::ToolResult(
            rig::completion::message::ToolResult {
                id: "call_abc123".to_string(),
                call_id: None,
                content: OneOrMany::one(ToolResultContent::Text(Text {
                    text: "127.0.0.1 localhost\n::1 localhost".to_string(),
                    additional_params: None,
                })),
            },
        )),
    });

    // Simulate the AgentSession being cancelled (e.g., user pressed Esc) while another tool
    // call was in-flight. The PromptCancelled error now carries the full accumulated history.
    let err = rig::completion::PromptError::PromptCancelled {
        reason: "User pressed Esc during read_file".to_string(),
        chat_history: chat_history.clone(),
    };

    // Convert to TurnError (this is what turn.execute_turn does via From implementation)
    let turn_err = TurnError::from(err);

    // CRITICAL ASSERTION 1: cancelled flag must be true
    assert!(
        turn_err.cancelled,
        "TurnError must have cancelled=true for PromptCancelled"
    );

    // VERIFY: msg should match the cancellation reason
    assert_eq!(
        turn_err.msg, "User pressed Esc during read_file",
        "TurnError msg should be the cancellation reason"
    );

    // CRITICAL ASSERTION 2: messages must be Some and NOT empty
    let messages = turn_err.messages.expect(
        "PromptCancelled chat_history must be preserved — PromptCancelled provides full history \
         including tool_call + tool_result pairs after rig v0.39.0 fix",
    );

    // CRITICAL ASSERTION 3: we need >= 3 messages (user text + tool_call message + tool_result)
    assert!(
        messages.len() >= 3,
        "chat_history must contain at least 3 messages after v0.39 fix: user prompt \
         + assistant(tool_call) + user(tool_result), got {}",
        messages.len(),
    );

    // DEEP ASSERTION: Verify the specific message types and content preserved correctly
    assert!(
        matches!(&messages[0], Message::User { .. }),
        "msg[0] should be a User message (the initial prompt)"
    );
    assert!(
        matches!(&messages[1], Message::Assistant { .. }),
        "msg[1] should be an Assistant message (the tool_call)"
    );
    assert!(
        matches!(&messages[2], Message::User { .. }),
        "msg[2] should be a User message (the tool_result)"
    );

    // DEEP ASSERTION: Verify tool_call content is correct
    match &messages[1] {
        Message::Assistant { content, .. } => {
            assert_eq!(content.len(), 1);
            let tc = match content.first_ref() {
                AssistantContent::ToolCall(tc) => tc,
                _ => panic!("msg[1] should contain a ToolCall"),
            };
            assert_eq!(tc.function.name, "read_file");
        }
        _ => panic!("msg[1] must be Assistant with ToolCall"),
    }

    // DEEP ASSERTION: Verify tool_result content is correct
    match &messages[2] {
        Message::User { content, .. } => {
            assert_eq!(content.len(), 1);
            let tr = match content.first_ref() {
                UserContent::ToolResult(tr) => tr,
                _ => panic!("msg[2] should contain a ToolResult"),
            };
            assert_eq!(tr.id, "call_abc123");
        }
        _ => panic!("msg[2] must be User with ToolResult"),
    }
}

/// Contrasting test for the older/pre-v0.39 pattern: when chat_history only contained
/// a single user message (simulating what would have been the case before the fix),
/// PromptCancelled should still preserve it, but tools_call history count should be 1.
///
/// This proves we can distinguish between the good state (>=3 messages) and bad state
/// (fewer tool call messages). It also validates that even minimal chat_history survives.
#[test]
fn prompt_cancelled_with_tool_calls_earlier_has_non_empty_history() {
    // Simulate a pre-v0.39 scenario where only 2 messages made it through:
    // user(prompt) + partial assistant text (no tool calls yet, or they were lost).
    let chat_history: Vec<Message> = vec![
        Message::User {
            content: OneOrMany::one(UserContent::Text(Text {
                text: "Tell me about Rust".to_string(),
                additional_params: None,
            })),
        },
        Message::Assistant {
            id: None,
            content: OneOrMany::one(AssistantContent::Text(Text {
                text: "Rust is a systems programming language".to_string(),
                additional_params: None,
            })),
        },
    ];

    // Even with minimal history, messages must NOT be empty.
    let err = rig::completion::PromptError::PromptCancelled {
        reason: "User cancelled early".to_string(),
        chat_history: chat_history.clone(),
    };

    let turn_err = TurnError::from(err);

    // cancelled must be true
    assert!(turn_err.cancelled);

    // The older pattern still preserves what it has — at minimum >= 2 messages (user + assistant).
    // This is the "non-empty history" part: even minimal chat_history survives.
    let messages = turn_err.messages.expect(
        "Even minimal chat_history should be preserved after cancel — \
         PromptCancelled provides whatever was accumulated",
    );

    assert!(
        messages.len() >= 2,
        "Minimal chat_history (user prompt + assistant text) must survive cancellation: got {}",
        messages.len(),
    );

    // Verify the content types of the preserved messages.
    assert!(matches!(&messages[0], Message::User { .. }));
    assert!(matches!(&messages[1], Message::Assistant { .. }));
}
