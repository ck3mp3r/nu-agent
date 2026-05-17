//! Conversion between completion Message types and our session Message type.
//!
//! This module handles conversion from completion messages (returned in agent responses)
//! to our session::Message type for persistence.

use crate::session::{Message, MessageRole, MessageUsage, StoredToolCall};
use rig::completion::message::{AssistantContent, Text, ToolResultContent, UserContent};
use rig::one_or_many::OneOrMany;

/// Convert a sequence of rig Messages from agent response into our session Messages.
///
/// # Arguments
///
/// * `messages` - Slice of completion Messages from the agent's prompt response
/// * `usage` - Optional usage statistics to attach to the last assistant message
///
/// # Returns
///
/// Vec of session::Message ready to be persisted
///
/// # Conversion Rules
///
/// - `Message::User` with `Text` → `Message` with `role=User`
/// - `Message::User` with `ToolResult` → `Message` with `role=Tool`
/// - `Message::Assistant` → `Message` with `role=Assistant`, combining text and tool_calls
/// - `Message::System` → `Message` with `role=System`
/// - Usage is applied to the last assistant message only
pub fn convert_messages(
    messages: &[rig::completion::Message],
    usage: Option<&rig::completion::request::Usage>,
) -> Vec<Message> {
    let mut result = Vec::new();

    for msg in messages {
        match msg {
            rig::completion::Message::User { content } => {
                for item in content.iter() {
                    match item {
                        UserContent::Text(text) => {
                            result.push(Message::new(MessageRole::User, text.text.clone()));
                        }
                        UserContent::ToolResult(tool_result) => {
                            // Convert tool result content to string
                            let content_str = tool_result_content_to_string(&tool_result.content);
                            let mut msg = Message::new(MessageRole::Tool, content_str.clone());
                            // Set tool_call_id so it can be matched with the assistant's tool call
                            msg = msg.with_tool_call_id(tool_result.id.clone());
                            msg = msg.with_tool_details(String::new(), content_str, true);
                            result.push(msg);
                        }
                        _ => {} // Image, etc. — skip for now
                    }
                }
            }
            rig::completion::Message::Assistant { content, .. } => {
                // Separate text and tool calls
                let mut texts = Vec::new();
                let mut tool_calls = Vec::new();

                for item in content.iter() {
                    match item {
                        AssistantContent::Text(text) => texts.push(text.text.clone()),
                        AssistantContent::ToolCall(tc) => {
                            tool_calls.push(StoredToolCall {
                                id: tc.id.clone(),
                                name: tc.function.name.clone(),
                                arguments: serde_json::to_string(&tc.function.arguments)
                                    .unwrap_or_default(),
                            });
                        }
                        _ => {} // Reasoning, etc.
                    }
                }

                let combined_text = texts.join("\n");
                let mut msg = Message::new(MessageRole::Assistant, combined_text);
                if !tool_calls.is_empty() {
                    msg = msg.with_tool_calls(tool_calls);
                }
                result.push(msg);
            }
            rig::completion::Message::System { content } => {
                result.push(Message::new(MessageRole::System, content.clone()));
            }
        }
    }

    // Apply usage to the last assistant message if available
    if let Some(usage) = usage
        && let Some(last_assistant) = result
            .iter_mut()
            .rev()
            .find(|m| m.role() == MessageRole::Assistant)
    {
        last_assistant.set_usage(MessageUsage::new(
            usage.input_tokens,
            usage.output_tokens,
            usage.total_tokens,
        ));
    }

    result
}

/// Convert ToolResultContent items to a combined string
fn tool_result_content_to_string(content: &OneOrMany<ToolResultContent>) -> String {
    content
        .iter()
        .map(|c| match c {
            ToolResultContent::Text(Text { text }) => text.clone(),
            _ => "[unsupported content]".to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[path = "persistence_test.rs"]
mod persistence_test;
