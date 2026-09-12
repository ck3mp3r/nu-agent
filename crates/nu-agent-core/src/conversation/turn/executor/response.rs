use super::core::TurnResponseData;
use crate::config::Config;
use crate::types::Message;
use nu_protocol::{Span, Value};

// ---------------------------------------------------------------------------
// TurnVisitor — module-level so it can be generic over P: AsyncPermissionResolver
// ---------------------------------------------------------------------------

/// If the last message in `messages` is a `User` message whose content
/// consists entirely of `ToolResult` items, appends a synthetic assistant
/// message to close the tool block. This prevents `user(ToolResult) →
/// user(Text)` on the next turn, which both Anthropic and OpenAI reject.
/// Returns the (possibly extended) message list.
pub fn close_open_tool_result_block(messages: Vec<Message>, error_msg: &str) -> Vec<Message> {
    let needs_closing = matches!(
        messages.last(),
        Some(Message::User { content }) if content.iter().all(|c| matches!(c, crate::types::UserContent::ToolResult(_)))
    );
    if needs_closing {
        log::debug!("close_open_tool_result_block: appending synthetic assistant");
        let mut msgs = messages;
        msgs.push(Message::assistant(format!("[Turn failed: {error_msg}]")));
        msgs
    } else {
        messages
    }
}

/// Build the final response `Value` from turn data. Called by the delegate after
/// auto-compaction has been evaluated.
pub fn build_response(
    response_data: Option<TurnResponseData>,
    config: &Config,
    session_id: Option<&str>,
    span: Span,
) -> Value {
    let data = response_data.unwrap_or(TurnResponseData {
        text: String::new(),
        usage: rig::completion::request::Usage {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
            tool_use_prompt_tokens: 0,
            reasoning_tokens: 0,
        },
        has_session: false,
    });

    let message_count = 0;

    let llm_response = crate::llm::LlmResponse {
        text: data.text,
        usage: crate::llm::LlmUsage {
            input_tokens: data.usage.input_tokens,
            output_tokens: data.usage.output_tokens,
            total_tokens: data.usage.total_tokens,
            cached_input_tokens: data.usage.cached_input_tokens,
            cache_creation_input_tokens: data.usage.cache_creation_input_tokens,
        },
        tool_calls: Vec::new(),         // TODO: track tool calls in TurnResult
        tool_call_metadata: Vec::new(), // TODO: track tool metadata in TurnResult
    };

    let response_value = crate::llm::format_response(&llm_response, config, session_id, span);

    if data.has_session
        && let Ok(record) = response_value.as_record()
    {
        let mut new_record = record.clone();
        if let Some(meta_value) = new_record.get("_meta")
            && let Ok(meta_record) = meta_value.as_record()
        {
            let mut new_meta = meta_record.clone();
            new_meta.insert(
                "message_count".to_string(),
                Value::int(message_count as i64, span),
            );

            new_record.insert("_meta".to_string(), Value::record(new_meta, span));
            return Value::record(new_record, span);
        }
    }

    response_value
}

/// Extract a "retry after N seconds" value from an error message string.
///
/// Matches common patterns from provider error responses:
/// - "retry after 5 seconds"
/// - "retry_after: 10"
/// - "Retry-After: 30"
///
/// Returns the value in milliseconds, or `None` if no recognisable pattern is found.
pub fn extract_retry_after_ms(msg: &str) -> Option<u64> {
    let msg_lower = msg.to_lowercase();
    // Pattern: "retry after N" or "retry-after: N" or "retry_after: N"
    let patterns = ["retry after ", "retry-after: ", "retry_after: "];
    for pattern in patterns {
        let Some(idx) = msg_lower.find(pattern) else {
            continue;
        };
        let after = &msg[idx + pattern.len()..];
        // Parse the first contiguous digits after the pattern
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            continue;
        }
        let Ok(seconds) = digits.parse::<u64>() else {
            continue;
        };
        return Some(seconds.saturating_mul(1000));
    }
    None
}
