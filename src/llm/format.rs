use crate::config::Config;
use crate::llm::LlmResponse;
use nu_protocol::{Span, Value};
use rig::completion::message::AssistantContent;

pub fn format_response(
    llm_response: &LlmResponse,
    config: &Config,
    session_id: Option<&str>,
    compaction_count: usize,
    span: Span,
) -> Value {
    use chrono::Utc;

    let timestamp = Utc::now().to_rfc3339();

    let usage_record = Value::record(
        vec![
            (
                "input_tokens".to_string(),
                Value::int(llm_response.usage.input_tokens as i64, span),
            ),
            (
                "output_tokens".to_string(),
                Value::int(llm_response.usage.output_tokens as i64, span),
            ),
            (
                "total_tokens".to_string(),
                Value::int(llm_response.usage.total_tokens as i64, span),
            ),
            (
                "cached_input_tokens".to_string(),
                Value::int(llm_response.usage.cached_input_tokens as i64, span),
            ),
            (
                "cache_creation_input_tokens".to_string(),
                Value::int(llm_response.usage.cache_creation_input_tokens as i64, span),
            ),
        ]
        .into_iter()
        .collect(),
        span,
    );

    let mut meta_fields = vec![];
    if let Some(id) = session_id {
        meta_fields.push(("session_id".to_string(), Value::string(id, span)));
    }

    meta_fields.push((
        "compacted".to_string(),
        Value::bool(compaction_count > 0, span),
    ));
    meta_fields.push((
        "compaction_count".to_string(),
        Value::int(compaction_count as i64, span),
    ));

    let tool_calls_list = build_tool_call_values(llm_response, span);
    meta_fields.push(("tool_calls".to_string(), Value::list(tool_calls_list, span)));
    meta_fields.push(("usage".to_string(), usage_record));

    let meta_record = Value::record(meta_fields.into_iter().collect(), span);

    Value::record(
        vec![
            (
                "response".to_string(),
                Value::string(&llm_response.text, span),
            ),
            ("model".to_string(), Value::string(&config.model, span)),
            (
                "provider".to_string(),
                Value::string(&config.provider, span),
            ),
            ("timestamp".to_string(), Value::string(timestamp, span)),
            ("_meta".to_string(), meta_record),
        ]
        .into_iter()
        .collect(),
        span,
    )
}

fn build_tool_call_values(llm_response: &LlmResponse, span: Span) -> Vec<Value> {
    if !llm_response.tool_call_metadata.is_empty() {
        return llm_response
            .tool_call_metadata
            .iter()
            .map(|metadata| {
                let mut fields = vec![
                    ("id".to_string(), Value::string(&metadata.id, span)),
                    ("name".to_string(), Value::string(&metadata.name, span)),
                    (
                        "arguments".to_string(),
                        Value::string(&metadata.arguments, span),
                    ),
                ];

                if let Some(source) = &metadata.source {
                    fields.push(("source".to_string(), Value::string(source, span)));
                }
                if let Some(error_kind) = &metadata.error_kind {
                    fields.push(("error_kind".to_string(), Value::string(error_kind, span)));
                }
                if let Some(message) = &metadata.message {
                    fields.push(("message".to_string(), Value::string(message, span)));
                }
                if let Some(details) = &metadata.details {
                    fields.push(("details".to_string(), Value::string(details, span)));
                }

                Value::record(fields.into_iter().collect(), span)
            })
            .collect();
    }

    llm_response
        .tool_calls
        .iter()
        .filter_map(|content| {
            if let AssistantContent::ToolCall(tool_call) = content {
                Some(Value::record(
                    vec![
                        ("id".to_string(), Value::string(&tool_call.id, span)),
                        (
                            "name".to_string(),
                            Value::string(&tool_call.function.name, span),
                        ),
                        (
                            "arguments".to_string(),
                            Value::string(
                                serde_json::to_string(&tool_call.function.arguments)
                                    .unwrap_or_default(),
                                span,
                            ),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                    span,
                ))
            } else {
                None
            }
        })
        .collect()
}
