//! Shared fixtures for the resolver test modules.
//!
//! Holds the imports and the helpers every topical resolver test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use crate::protocol::contracts::UiMessageSnapshot;
pub(super) use crate::types::{
    AssistantContent, AssistantMessage, CallId, Message, Text, ToolCall, ToolFunction, ToolName,
    ToolResult, ToolResultContent, UserContent,
};
pub(super) use serde_json::json;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Helper function to convert rig messages to UI snapshots for testing.
/// Delegates to the actual hydrate_single_message function in resolver.rs.
pub(super) fn convert_rig_messages_to_snapshots(messages: &[Message]) -> Vec<UiMessageSnapshot> {
    let tool_success_map = std::collections::HashMap::new();
    let tool_displays = std::collections::HashMap::new();
    messages
        .iter()
        .flat_map(|m| super::resolver::hydrate_single_message(m, &tool_success_map, &tool_displays))
        .collect()
}

// -- Test Support

/// Hydrate `messages` into transcript snapshots, as the resolver does.
pub(super) fn hydrate(messages: &[Message], cwd: &std::path::Path) -> Vec<UiMessageSnapshot> {
    super::resolver::hydrate_transcript_messages(messages, cwd)
}

/// Build [assistant ToolCall, user ToolResult] messages for one call whose
/// result text is `text` and whose persisted `is_error` flag is `is_error`.
pub(super) fn tool_call_entries(call_id: &str, text: &str, is_error: bool) -> Vec<Message> {
    vec![
        Message::Assistant(AssistantMessage::new(vec![AssistantContent::ToolCall(
            ToolCall::new(
                CallId::from_wire(call_id),
                ToolFunction::new(
                    ToolName::new("read_file").expect("should be non-empty"),
                    json!({"path": "/tmp/test.txt"}),
                ),
            ),
        )])),
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire(call_id),
                name: ToolName::new("read_file").expect("non-empty tool name"),
                content: vec![ToolResultContent::Text(Text::new(text))],
                is_error,
            })],
        },
    ]
}
