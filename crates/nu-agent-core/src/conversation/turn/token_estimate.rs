use crate::types::{AssistantContent, Message, ToolResultContent, UserContent};

/// Estimates token count using the chars/4 heuristic.
/// Conservative: under-counts code/JSON content (4 chars per token is optimistic
/// for dense text). Callers should use a threshold of 60–70% of the actual context
/// limit to account for this under-count. Intentionally has no tokenizer dependency.
pub fn estimate_token_count(messages: &[Message]) -> usize {
    let total_chars: usize = messages.iter().map(message_char_count).sum();
    // +4 overhead per message (matches tiktoken's per-message overhead)
    let overhead = messages.len() * 4;
    (total_chars / 4) + overhead
}

fn message_char_count(msg: &Message) -> usize {
    match msg {
        Message::User { content } => content
            .iter()
            .map(|c| match c {
                UserContent::Text(t) => t.text.len(),
                UserContent::ToolResult(tr) => tr
                    .content
                    .iter()
                    .map(|tc| match tc {
                        ToolResultContent::Text(t) => t.text.len(),
                        _ => 0,
                    })
                    .sum(),
                _ => 0,
            })
            .sum(),
        Message::Assistant { content, .. } => content
            .iter()
            .map(|c| match c {
                AssistantContent::Text(t) => t.text.len(),
                AssistantContent::ToolCall(tc) => {
                    tc.function.name.len()
                        + serde_json::to_string(&tc.function.arguments)
                            .unwrap_or_default()
                            .len()
                }
                _ => 0,
            })
            .sum(),
        Message::System { content } => content.len(),
    }
}

#[cfg(test)]
#[path = "token_estimate_test.rs"]
mod test;
