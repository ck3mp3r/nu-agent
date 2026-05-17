//! Tests for rig message persistence conversion

use super::*;
use crate::session::MessageRole;
use rig::completion::message::{
    AssistantContent, Text, ToolCall, ToolFunction, ToolResult, ToolResultContent, UserContent,
};
use rig::one_or_many::OneOrMany;

#[test]
fn converts_user_text_message() {
    let msgs = vec![rig::completion::Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "hello".to_string(),
        })),
    }];

    let result = convert_messages(&msgs, None);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].role(), MessageRole::User);
    assert_eq!(result[0].content(), "hello");
}

#[test]
fn converts_assistant_text_only() {
    let msgs = vec![rig::completion::Message::Assistant {
        id: None,
        content: OneOrMany::one(AssistantContent::Text(Text {
            text: "response text".to_string(),
        })),
    }];

    let result = convert_messages(&msgs, None);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].role(), MessageRole::Assistant);
    assert_eq!(result[0].content(), "response text");
    assert!(result[0].tool_calls().is_none());
}

#[test]
fn converts_assistant_with_tool_calls() {
    let tool_call = ToolCall::new(
        "call_123".to_string(),
        ToolFunction::new(
            "get_weather".to_string(),
            serde_json::json!({"city": "Boston"}),
        ),
    );

    let msgs = vec![rig::completion::Message::Assistant {
        id: None,
        content: OneOrMany::many(vec![
            AssistantContent::Text(Text {
                text: "Let me check the weather".to_string(),
            }),
            AssistantContent::ToolCall(tool_call),
        ])
        .expect("Should create OneOrMany from vec"),
    }];

    let result = convert_messages(&msgs, None);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].role(), MessageRole::Assistant);
    assert_eq!(result[0].content(), "Let me check the weather");

    let tool_calls = result[0].tool_calls().expect("Should have tool calls");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].id, "call_123");
    assert_eq!(tool_calls[0].name, "get_weather");
    assert!(tool_calls[0].arguments.contains("Boston"));
}

#[test]
fn converts_tool_result() {
    let tool_result = ToolResult {
        id: "call_123".to_string(),
        call_id: Some("call_123".to_string()),
        content: OneOrMany::one(ToolResultContent::Text(Text {
            text: "Weather is sunny".to_string(),
        })),
    };

    let msgs = vec![rig::completion::Message::User {
        content: OneOrMany::one(UserContent::ToolResult(tool_result)),
    }];

    let result = convert_messages(&msgs, None);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].role(), MessageRole::Tool);
    assert_eq!(result[0].content(), "Weather is sunny");
    assert_eq!(result[0].tool_call_id(), Some("call_123"));
}

#[test]
fn converts_system_message() {
    let msgs = vec![rig::completion::Message::System {
        content: "You are a helpful assistant".to_string(),
    }];

    let result = convert_messages(&msgs, None);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].role(), MessageRole::System);
    assert_eq!(result[0].content(), "You are a helpful assistant");
}

#[test]
fn applies_usage_to_last_assistant_message() {
    let msgs = vec![
        rig::completion::Message::User {
            content: OneOrMany::one(UserContent::Text(Text {
                text: "question".to_string(),
            })),
        },
        rig::completion::Message::Assistant {
            id: None,
            content: OneOrMany::one(AssistantContent::Text(Text {
                text: "answer".to_string(),
            })),
        },
    ];

    let usage = rig::completion::request::Usage {
        input_tokens: 10,
        output_tokens: 20,
        total_tokens: 30,
        cached_input_tokens: 0,
        cache_creation_input_tokens: 0,
    };

    let result = convert_messages(&msgs, Some(&usage));

    assert_eq!(result.len(), 2);

    // First message should not have usage
    assert!(result[0].usage().is_none());

    // Last assistant message should have usage
    let msg_usage = result[1].usage().expect("Should have usage");
    assert_eq!(msg_usage.input_tokens(), Some(10));
    assert_eq!(msg_usage.output_tokens(), Some(20));
    assert_eq!(msg_usage.total_tokens(), Some(30));
}

#[test]
fn converts_full_conversation() {
    // Simulate: User → Assistant(tool_call) → Tool(result) → Assistant(text)
    let tool_call = ToolCall::new(
        "call_123".to_string(),
        ToolFunction::new("search".to_string(), serde_json::json!({"query": "rust"})),
    );

    let tool_result = ToolResult {
        id: "call_123".to_string(),
        call_id: Some("call_123".to_string()),
        content: OneOrMany::one(ToolResultContent::Text(Text {
            text: "Found 10 results".to_string(),
        })),
    };

    let msgs = vec![
        rig::completion::Message::User {
            content: OneOrMany::one(UserContent::Text(Text {
                text: "Search for rust".to_string(),
            })),
        },
        rig::completion::Message::Assistant {
            id: None,
            content: OneOrMany::one(AssistantContent::ToolCall(tool_call)),
        },
        rig::completion::Message::User {
            content: OneOrMany::one(UserContent::ToolResult(tool_result)),
        },
        rig::completion::Message::Assistant {
            id: None,
            content: OneOrMany::one(AssistantContent::Text(Text {
                text: "I found 10 results for rust".to_string(),
            })),
        },
    ];

    let result = convert_messages(&msgs, None);

    assert_eq!(result.len(), 4);

    // User message
    assert_eq!(result[0].role(), MessageRole::User);
    assert_eq!(result[0].content(), "Search for rust");

    // Assistant with tool call
    assert_eq!(result[1].role(), MessageRole::Assistant);
    assert!(result[1].tool_calls().is_some());
    assert_eq!(result[1].tool_calls().unwrap()[0].name, "search");

    // Tool result
    assert_eq!(result[2].role(), MessageRole::Tool);
    assert_eq!(result[2].content(), "Found 10 results");
    assert_eq!(result[2].tool_call_id(), Some("call_123"));

    // Final assistant response
    assert_eq!(result[3].role(), MessageRole::Assistant);
    assert_eq!(result[3].content(), "I found 10 results for rust");
}

#[test]
fn handles_multiple_text_parts() {
    // Test that multiple Text parts in assistant message are combined
    let msgs = vec![rig::completion::Message::Assistant {
        id: None,
        content: OneOrMany::many(vec![
            AssistantContent::Text(Text {
                text: "First part".to_string(),
            }),
            AssistantContent::Text(Text {
                text: "Second part".to_string(),
            }),
        ])
        .expect("Should create OneOrMany from vec"),
    }];

    let result = convert_messages(&msgs, None);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].content(), "First part\nSecond part");
}

#[test]
fn handles_empty_messages_list() {
    let result = convert_messages(&[], None);
    assert_eq!(result.len(), 0);
}

#[test]
fn skips_unsupported_content_types() {
    // Test with mixed content including unsupported types
    let msgs = vec![rig::completion::Message::User {
        content: OneOrMany::many(vec![
            UserContent::Text(Text {
                text: "valid text".to_string(),
            }),
            // Image content would be skipped if we had it in the enum
        ])
        .expect("Should create OneOrMany from vec"),
    }];

    let result = convert_messages(&msgs, None);

    // Should only convert the text content
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].content(), "valid text");
}
