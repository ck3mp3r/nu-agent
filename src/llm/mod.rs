use crate::config::Config;
use crate::providers::github_copilot;
use nu_protocol::{LabeledError, Span, Value};
use rig::client::CompletionClient;
use rig::completion::message::AssistantContent;
use rig::providers::{anthropic, ollama, openai};

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
    })
}

/// Call an LLM with the given prompt and configuration.
///
/// Creates the appropriate provider client based on config.provider,
/// sends the prompt, and returns the response with usage statistics.
///
/// Unified implementation: creates provider-specific agent inline,
/// calls completion, and extracts response using shared helper.
///
/// Supports: openai, anthropic, ollama, github-copilot (use model format: "backend/model")
///
/// For github-copilot, endpoint/model-family routing is delegated to the
/// provider factory layer (`providers::github_copilot::factory`).
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
/// - Missing API key when required (checks env var if config.api_key is None)
/// - Unsupported provider
/// - Invalid configuration
/// - API errors (network, rate limits, etc.)
///
/// Helper to call completion on any agent implementing the Completion trait.
///
/// This eliminates duplication of the completion call logic across all providers.
/// Each provider creates their specific agent, then this function handles the
/// unified completion call and response extraction.
async fn call_agent_completion<M, A>(
    agent: A,
    prompt: &str,
    tools: Vec<rig::completion::ToolDefinition>,
) -> Result<LlmResponse, LabeledError>
where
    M: rig::completion::CompletionModel,
    A: rig::completion::Completion<M>,
{
    let response = agent
        .completion(prompt, Vec::<rig::completion::Message>::new())
        .await
        .map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))?
        .tools(tools)
        .send()
        .await
        .map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))?;

    extract_response(response)
}

pub async fn call_llm(
    config: &Config,
    prompt: &str,
    tools: Vec<rig::completion::ToolDefinition>,
) -> Result<LlmResponse, LabeledError> {
    // Match creates agent and calls helper, which returns unified LlmResponse
    match config.provider.as_str() {
        "openai" => {
            let key = config
                .api_key
                .clone()
                .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                .ok_or_else(|| LabeledError::new("Missing OPENAI_API_KEY"))?;
            let client = if let Some(url) = &config.base_url {
                openai::Client::builder()
                    .api_key(key)
                    .base_url(url.clone())
                    .build()
                    .map_err(|e| {
                        LabeledError::new(format!("Failed to create OpenAI client: {}", e))
                    })?
            } else {
                openai::Client::builder()
                    .api_key(key)
                    .build()
                    .map_err(|e| {
                        LabeledError::new(format!("Failed to create OpenAI client: {}", e))
                    })?
            };
            let agent = client.agent(&config.model).build();
            call_agent_completion(agent, prompt, tools).await
        }
        "anthropic" => {
            let key = config
                .api_key
                .clone()
                .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
                .ok_or_else(|| LabeledError::new("Missing ANTHROPIC_API_KEY"))?;
            let client = if let Some(url) = &config.base_url {
                anthropic::Client::builder()
                    .api_key(key)
                    .base_url(url.clone())
                    .build()
                    .map_err(|e| {
                        LabeledError::new(format!("Failed to create Anthropic client: {}", e))
                    })?
            } else {
                anthropic::Client::builder()
                    .api_key(key)
                    .build()
                    .map_err(|e| {
                        LabeledError::new(format!("Failed to create Anthropic client: {}", e))
                    })?
            };
            let agent = client.agent(&config.model).build();
            call_agent_completion(agent, prompt, tools).await
        }
        "ollama" => {
            let url = config
                .base_url
                .as_deref()
                .unwrap_or("http://localhost:11434");
            let client = ollama::Client::builder()
                .api_key(rig::client::Nothing)
                .base_url(url)
                .build()
                .map_err(|e| LabeledError::new(format!("Failed to create Ollama client: {}", e)))?;
            let agent = client.agent(&config.model).build();
            call_agent_completion(agent, prompt, tools).await
        }
        "github-copilot" => {
            use crate::providers::github_copilot::{Agent, ClientExt};
            let agent = github_copilot::Client::agent_from_config(
                "github-copilot",
                &config.model,
                config.api_key.clone(),
                config.base_url.clone(),
            )
            .map_err(|e| {
                LabeledError::new(format!("Failed to create GitHub Copilot agent: {}", e))
            })?;

            // Orchestration only: provider factory already selected backend+endpoint variant.
            match agent {
                Agent::Anthropic(agent) => call_agent_completion(agent, prompt, tools).await,
                Agent::OpenAI4x(agent) => call_agent_completion(agent, prompt, tools).await,
                Agent::OpenAI5x(agent) => call_agent_completion(agent, prompt, tools).await,
            }
        }
        other => Err(LabeledError::new(format!(
            "Unsupported provider: {}",
            other
        ))),
    }
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

    // Convert tool_calls to Nushell values
    let tool_calls_list: Vec<Value> = llm_response
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
        .collect();
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

#[cfg(test)]
mod tests;
