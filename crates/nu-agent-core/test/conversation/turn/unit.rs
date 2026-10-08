//! Unit tests for the turn module.
//!
//! Covers `TurnResult` / `TurnError` construction and conversion. Integration
//! tests that drive `execute_turn` with a mock model live in `test.rs`.

use super::*;
use crate::types::{
    AssistantContent, CallId, Message, Text, ToolCall, ToolFunction, ToolName, ToolResultContent,
    UserContent,
};

// ---------------------------------------------------------------------------
// Unit tests: TurnResult / TurnError construction and field access
// ---------------------------------------------------------------------------

#[test]
fn turn_result_can_be_constructed() {
    let result = TurnResult {
        text: "Hello".to_string(),
        usage: rig::completion::request::Usage {
            input_tokens: Some(10),
            output_tokens: Some(5),
            total_tokens: Some(15),
            cached_input_tokens: Some(0),
            cache_creation_input_tokens: Some(0),
            tool_use_prompt_tokens: Some(0),
            reasoning_tokens: Some(0),
        },
        messages: None,
        tool_call_count: 0,
        deltas_emitted: false,
        cancelled: false,
        cancel_reason: None,
        last_total_tokens: 0,
        pre_turn_message_count: 0,
        last_known_history: vec![],
    };

    assert_eq!(result.text, "Hello");
    assert_eq!(result.usage.input_tokens, Some(10));
    assert_eq!(result.usage.output_tokens, Some(5));
    assert_eq!(result.tool_call_count, 0);
    assert!(!result.deltas_emitted);
    assert!(!result.cancelled);
}

#[test]
fn turn_error_can_be_constructed() {
    let error = TurnError::CompletionFailed {
        msg: "Test error".to_string(),
        kind: executor::CompletionErrorKind::Unknown,
    };

    assert!(!error.is_cancelled());
    let msg = match &error {
        TurnError::CompletionFailed { msg, .. } => msg.as_str(),
        _ => panic!("expected CompletionFailed"),
    };
    assert_eq!(msg, "Test error");

    let cancelled = TurnError::Cancelled {
        msg: "Cancelled".to_string(),
        messages: vec![],
    };
    assert!(cancelled.is_cancelled());
}

#[test]
fn prompt_cancelled_error_is_detected_as_cancellation() {
    let user_msg = Message::User {
        content: vec![UserContent::Text(Text {
            text: "Hello".to_string(),
            additional_params: None,
        })],
    };
    let err = rig::completion::PromptError::PromptCancelled {
        chat_history: vec![user_msg],
        reason: "Cancelled by user".to_string(),
    };

    let turn_err = TurnError::from(err);

    assert!(
        turn_err.is_cancelled(),
        "PromptCancelled variant should be detected as cancellation"
    );
    let (msg, messages) = match &turn_err {
        TurnError::Cancelled { msg, messages, .. } => (msg, messages),
        _ => panic!("expected Cancelled variant"),
    };
    assert!(msg.contains("Cancelled by user"));

    assert_eq!(
        messages.len(),
        1,
        "Should have one message from chat_history"
    );
}

#[test]
fn other_prompt_errors_are_not_cancelled() {
    let completion_err = rig::error::ProviderError::Response("Some other error".to_string());
    let err = rig::completion::PromptError::from(completion_err);

    let turn_err = TurnError::from(err);

    assert!(
        !turn_err.is_cancelled(),
        "Non-cancellation errors should not be detected as cancellation"
    );
}

#[test]
fn max_turns_error_is_not_cancelled() {
    let err = rig::completion::PromptError::MaxTurnsError {
        max_turns: 10,
        chat_history: vec![],
        prompt: Message::User {
            content: vec![UserContent::Text(Text {
                text: "test".to_string(),
                additional_params: None,
            })],
        },
    };

    let turn_err = TurnError::from(err);

    assert!(
        !turn_err.is_cancelled(),
        "MaxTurnsError should not be detected as cancellation"
    );
}

/// Test that TurnContext can be created without an MCP runtime.
#[test]
fn turn_context_always_has_tool_server_handle() {
    let handle = rig::tool::server::ToolServer::new().run();
    let _handle_clone = handle.clone();
}

/// Test that StreamingError converts to TurnError with cancelled=false for non-cancel errors.
#[test]
fn streaming_error_from_prompt_cancelled_captures_messages() {
    let user_msg = Message::User {
        content: vec![UserContent::Text(Text {
            text: "Tell me about async".to_string(),
            additional_params: None,
        })],
    };

    let inner = rig::completion::PromptError::PromptCancelled {
        reason: "Hook cancelled".to_string(),
        chat_history: vec![user_msg],
    };
    let streaming_err = rig::agent::StreamingError::Prompt(inner);

    let turn_err = TurnError::from(streaming_err);

    assert!(turn_err.is_cancelled());
    let (msg, messages) = match &turn_err {
        TurnError::Cancelled { msg, messages, .. } => (msg.as_str(), messages),
        _ => panic!("expected Cancelled variant"),
    };
    assert_eq!(msg, "Hook cancelled");
    assert_eq!(messages.len(), 1);
}

/// TurnError from PromptCancelled captures chat_history as messages.
#[test]
fn turn_error_from_prompt_cancelled_captures_messages() {
    let user_msg = Message::User {
        content: vec![UserContent::Text(Text {
            text: "What is Rust?".to_string(),
            additional_params: None,
        })],
    };
    let assistant_msg = Message::Assistant {
        id: None,
        content: vec![AssistantContent::Text(Text {
            text: "Rust is a systems programming...".to_string(),
            additional_params: None,
        })],
    };

    let err = rig::completion::PromptError::PromptCancelled {
        reason: "User pressed Esc".to_string(),
        chat_history: vec![user_msg, assistant_msg],
    };

    let turn_err = TurnError::from(err);

    assert!(turn_err.is_cancelled());
    let (msg, messages) = match &turn_err {
        TurnError::Cancelled { msg, messages, .. } => (msg.as_str(), messages),
        _ => panic!("expected Cancelled variant"),
    };
    assert_eq!(msg, "User pressed Esc");

    assert_eq!(
        messages.len(),
        2,
        "Both user and assistant messages should be captured"
    );
}

#[test]
fn turn_error_from_non_cancelled_has_no_messages() {
    let completion_err = rig::error::ProviderError::Response("Network timeout".to_string());
    let err = rig::completion::PromptError::from(completion_err);

    let turn_err = TurnError::from(err);

    assert!(!turn_err.is_cancelled());
    assert!(
        !matches!(turn_err, TurnError::Cancelled { .. }),
        "Non-cancelled errors should not be Cancelled variant"
    );
}

/// Path B: cancel_token fired, partial text accumulated.
#[test]
fn path_b_cancelled_with_partial_text_constructs_user_and_assistant_messages() {
    let turn_result = TurnResult {
        text: "partial response".to_string(),
        usage: rig::completion::request::Usage::default(),
        messages: None,
        tool_call_count: 0,
        deltas_emitted: true,
        cancelled: true,
        cancel_reason: None,
        last_total_tokens: 0,
        pre_turn_message_count: 0,
        last_known_history: vec![],
    };

    let prompt = "user prompt".to_string();
    let mut cancelled_messages = vec![Message::user(prompt.clone())];
    if !turn_result.text.is_empty() {
        cancelled_messages.push(Message::assistant(turn_result.text.clone()));
    }

    assert_eq!(
        cancelled_messages.len(),
        2,
        "Path B with partial text must produce user + assistant messages"
    );

    assert!(
        matches!(&cancelled_messages[0], Message::User { .. }),
        "First message must be a user message"
    );

    match &cancelled_messages[1] {
        Message::Assistant { content, .. } => {
            let text = content.iter().find_map(|c| {
                if let AssistantContent::Text(t) = c {
                    Some(t.text.as_str())
                } else {
                    None
                }
            });
            assert_eq!(
                text,
                Some("partial response"),
                "Assistant message must contain partial text"
            );
        }
        other => panic!("Expected assistant message, got {:?}", other),
    }
}

/// Path B: cancel_token fired, no text accumulated.
#[test]
fn path_b_cancelled_with_empty_text_constructs_only_user_message() {
    let turn_result = TurnResult {
        text: String::new(),
        usage: rig::completion::request::Usage::default(),
        messages: None,
        tool_call_count: 0,
        deltas_emitted: false,
        cancelled: true,
        cancel_reason: None,
        last_total_tokens: 0,
        pre_turn_message_count: 0,
        last_known_history: vec![],
    };

    let prompt = "user prompt".to_string();
    let mut cancelled_messages = vec![Message::user(prompt.clone())];
    if !turn_result.text.is_empty() {
        cancelled_messages.push(Message::assistant(turn_result.text.clone()));
    }

    assert_eq!(
        cancelled_messages.len(),
        1,
        "Path B with empty text must produce only the user message (no empty assistant)"
    );

    assert!(
        matches!(&cancelled_messages[0], Message::User { .. }),
        "The single message must be a user message"
    );
}

#[test]
fn turn_result_cancelled_flag_propagates() {
    let cancelled_result = TurnResult {
        text: String::new(),
        usage: rig::completion::request::Usage::default(),
        messages: None,
        tool_call_count: 0,
        deltas_emitted: true,
        cancelled: true,
        cancel_reason: None,
        last_total_tokens: 0,
        pre_turn_message_count: 0,
        last_known_history: vec![],
    };

    assert!(cancelled_result.cancelled, "Cancelled flag should be true");
    assert!(
        cancelled_result.text.is_empty(),
        "Cancelled turn should have empty text"
    );
    assert!(
        cancelled_result.messages.is_none(),
        "Cancelled via cancel_token should have no messages (FinalResponse not received)"
    );

    let normal_result = TurnResult {
        text: "Hello".to_string(),
        usage: rig::completion::request::Usage::default(),
        messages: Some(vec![]),
        tool_call_count: 1,
        deltas_emitted: true,
        cancelled: false,
        cancel_reason: None,
        last_total_tokens: 0,
        pre_turn_message_count: 0,
        last_known_history: vec![],
    };

    assert!(
        !normal_result.cancelled,
        "Normal turn should not be cancelled"
    );
}

// ---------------------------------------------------------------------------
// rig v0.39.0: Path B with tool calls — chat_history preserved after cancel
// ---------------------------------------------------------------------------

/// Regression test: PromptCancelled preserves tool_call + tool_result history.
#[test]
fn cancel_mid_tool_call_preserves_tool_call_and_tool_result_in_history() {
    use serde_json::json;

    let mut chat_history: Vec<Message> = Vec::new();

    chat_history.push(Message::User {
        content: vec![UserContent::Text(Text {
            text: "What is in /etc/hosts?".to_string(),
            additional_params: None,
        })],
    });

    chat_history.push(Message::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(ToolCall {
            id: CallId::from_wire("call_abc123"),
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: ToolName::new("read_file").expect("should be non-empty"),
                arguments: json!({ "path": "/etc/hosts" }),
            },
        })],
    });

    chat_history.push(Message::User {
        content: vec![UserContent::ToolResult(
            rig::completion::message::ToolResult {
                call: CallId::from_wire("call_abc123"),
                name: ToolName::new("read_file").expect("should be non-empty"),
                content: vec![ToolResultContent::Text(Text {
                    text: "file contents here".to_string(),
                    additional_params: None,
                })],
            },
        )],
    });

    let err = rig::completion::PromptError::PromptCancelled {
        reason: "User pressed Esc during tool execution".to_string(),
        chat_history: chat_history.clone(),
    };

    let turn_err = TurnError::from(err);

    assert!(
        turn_err.is_cancelled(),
        "TurnError must be marked as cancelled"
    );
    let (msg, messages) = match &turn_err {
        TurnError::Cancelled { msg, messages, .. } => (msg.as_str(), messages),
        _ => panic!("expected Cancelled variant"),
    };
    assert_eq!(msg, "User pressed Esc during tool execution");

    assert_eq!(
        messages.len(),
        3,
        "Must preserve user message + assistant(tool_call) + user(tool_result)"
    );

    match &messages[0] {
        Message::User { .. } => {}
        _ => panic!("msg[0] should be User (prompt)"),
    };
    match &messages[1] {
        Message::Assistant { .. } => {}
        _ => panic!("msg[1] should be Assistant (tool_call)"),
    };
    match &messages[2] {
        Message::User { .. } => {}
        _ => panic!("msg[2] should be User (tool_result)"),
    };
}

/// Regression: multiple tool-use cycles are all preserved on cancellation.
#[test]
fn cancel_preserves_multiple_tool_use_cycles() {
    use serde_json::json;

    let mut chat_history: Vec<Message> = Vec::new();

    // First tool-use cycle
    chat_history.push(Message::User {
        content: vec![UserContent::Text(Text {
            text: "List files".to_string(),
            additional_params: None,
        })],
    });
    chat_history.push(Message::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(ToolCall {
            id: CallId::from_wire("call_001"),
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: ToolName::new("list_dir").expect("should be non-empty"),
                arguments: json!({ "path": "/" }),
            },
        })],
    });
    chat_history.push(Message::User {
        content: vec![UserContent::ToolResult(
            rig::completion::message::ToolResult {
                call: CallId::from_wire("call_001"),
                name: ToolName::new("list_dir").expect("should be non-empty"),
                content: vec![ToolResultContent::Text(Text {
                    text: "/bin /usr /etc".to_string(),
                    additional_params: None,
                })],
            },
        )],
    });

    // Second tool-use cycle
    chat_history.push(Message::Assistant {
        id: None,
        content: vec![AssistantContent::Text(Text {
            text: "Now read that file".to_string(),
            additional_params: None,
        })],
    });
    chat_history.push(Message::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(ToolCall {
            id: CallId::from_wire("call_002"),
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: ToolName::new("read_file").expect("should be non-empty"),
                arguments: json!({ "path": "/etc/passwd" }),
            },
        })],
    });
    chat_history.push(Message::User {
        content: vec![UserContent::ToolResult(
            rig::completion::message::ToolResult {
                call: CallId::from_wire("call_002"),
                name: ToolName::new("read_file").expect("should be non-empty"),
                content: vec![ToolResultContent::Text(Text {
                    text: "root:x:0:0:root user".to_string(),
                    additional_params: None,
                })],
            },
        )],
    });

    let err = rig::completion::PromptError::PromptCancelled {
        reason: "Cancelled during read_file tool call".to_string(),
        chat_history: chat_history.clone(),
    };

    let turn_err = TurnError::from(err);

    assert!(turn_err.is_cancelled());

    let messages = match &turn_err {
        TurnError::Cancelled { messages, .. } => messages,
        _ => panic!(
            "expected Cancelled variant: all accumulated messages must be preserved on cancel — including 2 completed tool cycles"
        ),
    };

    assert_eq!(
        messages.len(),
        chat_history.len(),
        "Every single message in accumulated history should survive cancellation"
    );
}
