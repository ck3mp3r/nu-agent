use crate::session::StoreEntry;
use crate::types::{CallId, Message, Text, ToolName, ToolResult, ToolResultContent, UserContent};

use super::Result;

// -- Test Support

/// Build a ToolResult for `call_id` with one Text block holding `text` (no
/// additional params).
pub(super) fn tool_result(call_id: &str, text: &str) -> Result<UserContent> {
    Ok(UserContent::ToolResult(ToolResult {
        call: CallId::from_wire(call_id),
        name: ToolName::new("test_tool").map_err(|_| "non-empty tool name")?,
        content: vec![ToolResultContent::Text(Text::new(text))],
        is_error: false,
    }))
}

/// Build a User message carrying one ToolResult for `call_id` with one Text
/// block holding `text` (no additional params).
pub(super) fn tool_result_message(call_id: &str, text: &str) -> Result<Message> {
    Ok(Message::User {
        content: vec![tool_result(call_id, text)?],
    })
}

/// Read the success verdict from the first ToolResult in a store entry.
/// `None` when the entry is not a User message carrying a ToolResult.
pub(super) fn tool_result_flag(entry: &StoreEntry) -> Option<bool> {
    let StoreEntry::Message(Message::User { content }) = entry else {
        return None;
    };
    let Some(UserContent::ToolResult(tr)) = content.first() else {
        return None;
    };
    Some(!tr.is_error)
}

/// Read the success verdict from EACH ToolResult in a store entry, in order.
/// Content items that are not ToolResults are skipped.
pub(super) fn tool_result_flags(entry: &StoreEntry) -> Vec<Option<bool>> {
    let StoreEntry::Message(Message::User { content }) = entry else {
        return Vec::new();
    };
    content
        .iter()
        .filter_map(|c| {
            let UserContent::ToolResult(tr) = c else {
                return None;
            };
            Some(Some(!tr.is_error))
        })
        .collect()
}
