//! Shared fixtures for the repair test modules.
//!
//! Holds the imports and the helpers every topical repair test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use crate::types::{
    AssistantContent, AssistantMessage, CallId, Message, ToolCall, ToolFunction, ToolName,
    ToolResult, UserContent,
};

pub(super) fn make_tool_call(id: &str, name: &str) -> AssistantContent {
    AssistantContent::ToolCall(ToolCall::new(
        CallId::from_wire(id),
        ToolFunction::new(
            ToolName::new(name).expect("non-empty tool name"),
            serde_json::json!({}),
        ),
    ))
}

pub(super) fn make_tool_result(id: &str, output: &str) -> UserContent {
    UserContent::ToolResult(ToolResult {
        call: CallId::from_wire(id),
        name: ToolName::new("read_file").expect("non-empty tool name"),
        content: vec![crate::types::ToolResultContent::text(output)],
        is_error: false,
    })
}

pub(super) fn assistant_with_content(items: Vec<AssistantContent>) -> Message {
    Message::Assistant(AssistantMessage::new(items))
}

pub(super) fn user_with_content(items: Vec<UserContent>) -> Message {
    Message::User { content: items }
}

pub(super) fn make_invalid_args_tool_call(id: &str, name: &str) -> AssistantContent {
    AssistantContent::ToolCall(ToolCall::new(
        CallId::from_wire(id),
        ToolFunction {
            name: ToolName::new(name).expect("non-empty tool name"),
            arguments: serde_json::Map::new(),
            invalid_arguments: Some("not an object".to_string()),
        },
    ))
}

/// The compaction summary artifact is spliced as a leading `Message::User`
/// whose `Text` block carries the compaction summary header.
pub(super) fn summary_artifact(summary: &str) -> Message {
    Message::User {
        content: vec![UserContent::Text(crate::types::Text::new(format!(
            "{}{summary}",
            crate::conversation::compaction::compactor::SUMMARY_HEADER
        )))],
    }
}

/// True if any `UserContent` block in `content` carries the compaction
/// summary header.
pub(super) fn is_summary_content(content: &[UserContent]) -> bool {
    let header = crate::conversation::compaction::compactor::SUMMARY_HEADER;
    content.iter().any(|c| match c {
        UserContent::Text(t) => t.text.starts_with(header),
        _ => false,
    })
}
