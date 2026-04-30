use crate::config::Config;
use crate::plugin::RuntimeCtx;
use nu_protocol::{LabeledError, Span, Value};
use rig::completion::message::AssistantContent;

pub mod runtime;

/// Token usage statistics from LLM response.
///
/// Decoupled from rig's Usage type to:
/// - Simplify testing (no dependency on rig in tests)
/// - Enable future extensions (e.g., cost estimation)
/// - Clean conversion to Nushell records
#[derive(Debug, Clone, PartialEq)]
pub struct LlmUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
}

impl From<rig::completion::request::Usage> for LlmUsage {
    fn from(usage: rig::completion::request::Usage) -> Self {
        Self {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            cache_creation_input_tokens: usage.cache_creation_input_tokens,
        }
    }
}

/// LLM response with text, usage statistics, and tool calls.
///
/// Returned by call_llm() to provide the response text, token usage information,
/// and any tool calls requested by the LLM.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmResponse {
    pub text: String,
    pub usage: LlmUsage,
    pub tool_calls: Vec<AssistantContent>,
    pub tool_call_metadata: Vec<ToolCallMetadata>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallMetadata {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub source: Option<String>,
}

/// Extract response data from rig's CompletionResponse.
///
/// Processes the completion response choice to extract:
/// - Text content (joined with newlines)
/// - Tool calls (if any)
/// - Usage statistics (converted to LlmUsage)
///
/// Ignores other content types like Reasoning and Image.
///
/// # Generic Parameter
/// * `T` - The raw response type from the provider (e.g., OpenAI, Anthropic, Ollama)
///
/// # Returns
/// * `Ok(LlmResponse)` - Extracted response data
/// * `Err(LabeledError)` - Should not error in practice, but allows for future validation
pub(crate) fn extract_response<T>(
    completion_response: rig::completion::CompletionResponse<T>,
) -> Result<LlmResponse, LabeledError> {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for content in completion_response.choice {
        match content {
            AssistantContent::Text(t) => {
                text_parts.push(t.to_string());
            }
            tool_call @ AssistantContent::ToolCall(_) => {
                tool_calls.push(tool_call);
            }
            _ => {
                // Ignore Reasoning, Image, and any future variants
            }
        }
    }

    Ok(LlmResponse {
        text: text_parts.join("\n"),
        usage: completion_response.usage.into(),
        tool_calls,
        tool_call_metadata: vec![],
    })
}

/// Call an LLM with the given prompt and configuration.
///
/// Orchestration only: delegates provider lifecycle and execution to [`LlmRuntime`].
///
/// This function intentionally does **not**:
/// - construct provider clients
/// - branch by endpoint/model-family
/// - implement provider-specific payload parsing
///
/// # Arguments
/// * `config` - Configuration with provider, model, and auth details
/// * `prompt` - The prompt to send to the LLM
/// * `tools` - Tool definitions to pass to the LLM (empty vec if no tools)
///
/// # Returns
/// The LLM response with text and usage statistics, or error if the call fails
///
/// # Errors
/// - Missing API key when required (resolved by runtime/provider factory)
/// - Unsupported provider
/// - Invalid configuration
/// - API errors (network, rate limits, etc.)
///
pub async fn call_llm(
    runtime_ctx: &RuntimeCtx,
    config: &Config,
    prompt: &str,
    tools: Vec<rig::completion::ToolDefinition>,
) -> Result<LlmResponse, LabeledError> {
    runtime_ctx.llm_runtime().call(config, prompt, tools).await
}

/// Format LLM response as a Nushell Value::Record.
///
/// Creates a record with the following fields:
/// - `response`: The LLM response text (String)
/// - `model`: The model used (String)
/// - `provider`: The provider used (String)
/// - `timestamp`: ISO8601 timestamp of when the response was created (String)
/// - `_meta`: Metadata record containing:
///   - `session_id`: Session identifier (String, optional - only included if Some)
///   - `compacted`: Whether context has been compacted (Bool, derived from compaction_count > 0)
///   - `compaction_count`: Number of times context was compacted (Int)
///   - `tool_calls`: List of tool calls made (List, default empty)
///   - `usage`: Token usage statistics record containing:
///     - `input_tokens`: Input tokens used (Int)
///     - `output_tokens`: Output tokens used (Int)
///     - `total_tokens`: Total tokens used (Int)
///     - `cached_input_tokens`: Cached input tokens (Int)
///     - `cache_creation_input_tokens`: Cache creation tokens (Int)
///
/// # Arguments
/// * `llm_response` - The LLM response with text and usage
/// * `config` - The configuration used for the request
/// * `session_id` - Optional session identifier
/// * `compaction_count` - Number of context compactions
/// * `span` - The span for the Value
///
/// # Returns
/// A Value::Record containing the response and metadata
pub fn format_response(
    llm_response: &LlmResponse,
    config: &Config,
    session_id: Option<&str>,
    compaction_count: usize,
    span: Span,
) -> Value {
    use chrono::Utc;

    let timestamp = Utc::now().to_rfc3339();

    // Build usage record
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

    // Build _meta record fields
    let mut meta_fields = vec![];

    // Add session_id only if provided
    if let Some(id) = session_id {
        meta_fields.push(("session_id".to_string(), Value::string(id, span)));
    }

    // Add compaction metadata
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

    // Add usage record
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
    // Prefer enriched metadata when available; otherwise preserve legacy mapping from raw tool_calls.
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

#[cfg(test)]
mod tests;
