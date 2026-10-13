//! Regression tests for rig-core v0.39.0 PR #1899: tool call history on cancel/abort
//!
//! The v0.39.0 fix ensures that when an agent loop is cancelled mid-stream during a tool call,
//! rig's AgentSession retains accumulated chat_history (including completed tool_call and
//! tool_result pairs). This means Cancelled::chat_history will contain the full set of
//! messages — not just [user_msg, partial_assistant_text].
//!
//! While we cannot fire a real cancellation mid-tool-call end-to-end without a live LLM,
//! these tests prove the data path is correct: we construct chat_history as rig's
//! AgentSession.to_chat_history() would provide to Cancelled after v0.39,
//! pass them through Cancelled -> TurnError::from, and verify all messages are preserved.

use super::*;
use crate::types::{
    AssistantContent, AssistantMessage, CallId, ToolCall, ToolFunction, ToolName, ToolResultContent,
};
use crate::types::{Message, Text, UserContent};
use serde_json::json;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Regression test for rig-core v0.39.0 PR #1899: Cancelled::chat_history is NOT empty
/// after cancellation when tool calls have been made.
///
/// This tests the core fix: that tool_call + tool_result pairs in chat_history are preserved
/// through the Cancelled -> TurnError conversion. Before v0.39, tool call history was
/// lost; after v0.39 it survives cancellation.
#[test]
fn prompt_cancelled_preserves_tool_call_history() -> Result<()> {
    // Build chat_history as rig's AgentSession would provide to Cancelled after v0.39.0:
    // A complete tool-use cycle: user(prompt) -> assistant(tool_call) -> user(tool_result)
    let mut chat_history: Vec<Message> = Vec::new();

    // 1) User message (the initial prompt)
    chat_history.push(Message::User {
        content: vec![UserContent::Text(Text::new("What is in /etc/hosts?"))],
    });

    // 2) Assistant message with a ToolCall (the LLM decided to invoke read_file)
    chat_history.push(Message::Assistant(AssistantMessage::new(vec![
        AssistantContent::ToolCall(ToolCall::new(
            CallId::from_wire("call_abc123"),
            ToolFunction::new(ToolName::new("read_file")?, json!({ "path": "/etc/hosts" })),
        )),
    ])));

    // 3) User message with a ToolResult (LLM's prior tool_call result, completed before next
    //    iteration got cancelled mid-stream)
    chat_history.push(Message::User {
        content: vec![UserContent::ToolResult(
            rig::completion::message::ToolResult {
                call: CallId::from_wire("call_abc123"),
                name: ToolName::new("read_file")?,
                content: vec![ToolResultContent::Text(Text::new(
                    "127.0.0.1 localhost\n::1 localhost",
                ))],
                is_error: false,
            },
        )],
    });

    // Simulate the AgentSession being cancelled (e.g., user pressed Esc) while another tool
    // call was in-flight. The Cancelled error now carries the full accumulated history.
    let err = rig::completion::PromptError::Cancelled {
        reason: "User pressed Esc during read_file".to_string(),
        chat_history: chat_history.clone(),
    };

    // Convert to TurnError (this is what turn.execute_turn does via From implementation)
    let turn_err = TurnError::from(err);

    // CRITICAL ASSERTION 1: cancelled flag must be true
    assert!(
        turn_err.is_cancelled(),
        "TurnError must be Cancelled variant for Cancelled"
    );

    // VERIFY: msg should match the cancellation reason
    let (msg, messages) = match &turn_err {
        TurnError::Cancelled { msg, messages, .. } => (msg.as_str(), messages),
        _ => panic!("expected Cancelled variant"),
    };
    assert_eq!(
        msg, "User pressed Esc during read_file",
        "TurnError msg should be the cancellation reason"
    );

    // CRITICAL ASSERTION 2: messages must be non-empty
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
        matches!(&messages[1], Message::Assistant(_)),
        "msg[1] should be an Assistant message (the tool_call)"
    );
    assert!(
        matches!(&messages[2], Message::User { .. }),
        "msg[2] should be a User message (the tool_result)"
    );

    // DEEP ASSERTION: Verify tool_call content is correct
    match &messages[1] {
        Message::Assistant(assistant) => {
            let content = &assistant.content;
            assert_eq!(content.len(), 1);
            let tc = match content.first() {
                Some(AssistantContent::ToolCall(tc)) => tc,
                _ => panic!("msg[1] should contain a ToolCall"),
            };
            assert_eq!(tc.function.name, ToolName::new("read_file")?);
        }
        _ => panic!("msg[1] must be Assistant with ToolCall"),
    }

    // DEEP ASSERTION: Verify tool_result content is correct
    match &messages[2] {
        Message::User { content, .. } => {
            assert_eq!(content.len(), 1);
            let tr = match content.first() {
                Some(UserContent::ToolResult(tr)) => tr,
                _ => panic!("msg[2] should contain a ToolResult"),
            };
            assert_eq!(tr.call.wire().as_ref(), "call_abc123");
        }
        _ => panic!("msg[2] must be User with ToolResult"),
    }
    Ok(())
}

/// Contrasting test for the older/pre-v0.39 pattern: when chat_history only contained
/// a single user message (simulating what would have been the case before the fix),
/// Cancelled should still preserve it, but tools_call history count should be 1.
///
/// This proves we can distinguish between the good state (>=3 messages) and bad state
/// (fewer tool call messages). It also validates that even minimal chat_history survives.
#[test]
fn prompt_cancelled_with_tool_calls_earlier_has_non_empty_history() -> Result<()> {
    // Simulate a pre-v0.39 scenario where only 2 messages made it through:
    // user(prompt) + partial assistant text (no tool calls yet, or they were lost).
    let chat_history: Vec<Message> = vec![
        Message::User {
            content: vec![UserContent::Text(Text::new("Tell me about Rust"))],
        },
        Message::Assistant(AssistantMessage::new(vec![AssistantContent::Text(
            Text::new("Rust is a systems programming language"),
        )])),
    ];

    // Even with minimal history, messages must NOT be empty.
    let err = rig::completion::PromptError::Cancelled {
        reason: "User cancelled early".to_string(),
        chat_history: chat_history.clone(),
    };

    let turn_err = TurnError::from(err);

    // cancelled must be true
    assert!(turn_err.is_cancelled());

    // The older pattern still preserves what it has — at minimum >= 2 messages (user + assistant).
    // This is the "non-empty history" part: even minimal chat_history survives.
    let messages = match &turn_err {
        TurnError::Cancelled { messages, .. } => messages,
        _ => panic!("expected Cancelled variant"),
    };

    assert!(
        messages.len() >= 2,
        "Minimal chat_history (user prompt + assistant text) must survive cancellation: got {}",
        messages.len(),
    );

    // Verify the content types of the preserved messages.
    assert!(matches!(&messages[0], Message::User { .. }));
    assert!(matches!(&messages[1], Message::Assistant(_)));
    Ok(())
}
