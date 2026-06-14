use crate::types::{AssistantContent, Message, UserContent};

/// Returns true if the message is an Assistant message containing at least one ToolCall.
pub(crate) fn has_tool_call(msg: &Message) -> bool {
    match msg {
        Message::Assistant { content, .. } => content
            .iter()
            .any(|c| matches!(c, AssistantContent::ToolCall(_))),
        _ => false,
    }
}

/// Returns true if the message is a User message containing at least one ToolResult.
pub(crate) fn has_tool_result(msg: &Message) -> bool {
    match msg {
        Message::User { content } => content
            .iter()
            .any(|c| matches!(c, UserContent::ToolResult(_))),
        _ => false,
    }
}

/// Adjusts a target split index so that it never falls between a ToolCall and its
/// corresponding ToolResult. If the boundary would separate a pair, it moves backward
/// until the boundary is safe.
pub(crate) fn find_safe_split_index(messages: &[Message], target_index: usize) -> usize {
    if target_index >= messages.len() {
        return messages.len();
    }
    if target_index == 0 {
        return 0;
    }
    let mut idx = target_index;
    loop {
        if idx == 0 {
            break;
        }
        if has_tool_result(&messages[idx]) || has_tool_call(&messages[idx - 1]) {
            idx -= 1;
        } else {
            break;
        }
    }
    idx
}

/// Estimates the token count for a message using a simple chars/4 heuristic.
pub(crate) fn estimate_tokens(msg: &Message) -> usize {
    let text = serde_json::to_string(msg).unwrap_or_default();
    text.len() / 4
}
