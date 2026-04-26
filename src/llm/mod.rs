use crate::config::Config;
use crate::providers::github_copilot;
use nu_protocol::{LabeledError, Span, Value};
use rig::client::{CompletionClient, ProviderClient};
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

/// Routing decision for which provider to use.
#[derive(Debug, PartialEq)]
pub(crate) enum ProviderRoute {
    OpenAI,
    Anthropic,
    Ollama,
    GitHubCopilot { backend: String },
    Unsupported(String),
}

/// Determine which provider to route to based on config.
///
/// Pure function: no I/O, no HTTP, no rig client creation.
pub(crate) fn route_provider(config: &Config) -> ProviderRoute {
    // 3-part format: "github-copilot/backend"
    if config.provider.starts_with("github-copilot/") {
        let backend = config
            .provider
            .strip_prefix("github-copilot/")
            .unwrap_or("")
            .to_string();
        return ProviderRoute::GitHubCopilot { backend };
    }

    // Legacy routing: use provider_impl or provider name
    let provider_name = config.provider_impl.as_deref().unwrap_or(&config.provider);

    match provider_name {
        // Legacy: "openai" or "github-copilot" with a githubcopilot base_url
        "github-copilot" | "openai"
            if config
                .base_url
                .as_ref()
                .is_some_and(|u| u.contains("githubcopilot")) =>
        {
            ProviderRoute::GitHubCopilot {
                backend: String::new(),
            }
        }
        "openai" => ProviderRoute::OpenAI,
        "anthropic" => ProviderRoute::Anthropic,
        "ollama" => ProviderRoute::Ollama,
        other => ProviderRoute::Unsupported(other.to_string()),
    }
}

/// Parse the backend name from a "github-copilot/<backend>" provider string.
///
/// Pure function: no I/O.
pub(crate) fn parse_github_copilot_backend(provider: &str) -> Result<&str, LabeledError> {
    provider
        .strip_prefix("github-copilot/")
        .filter(|b| !b.is_empty())
        .ok_or_else(|| LabeledError::new("Invalid GitHub Copilot provider format"))
}

/// Resolve the API key for a given provider.
///
/// Returns the config api_key if set, otherwise reads the appropriate env var.
/// Special case: github-copilot reads GITHUB_TOKEN instead of GITHUB-COPILOT_API_KEY.
///
/// Pure function w.r.t. config; reads env vars.
pub(crate) fn resolve_api_key(config: &Config, provider: &str) -> Result<String, LabeledError> {
    if let Some(ref key) = config.api_key {
        return Ok(key.clone());
    }

    let env_var = if provider == "github-copilot" || provider.starts_with("github-copilot/") {
        "GITHUB_TOKEN".to_string()
    } else {
        format!("{}_API_KEY", provider.to_uppercase())
    };

    std::env::var(&env_var).map_err(|_| {
        LabeledError::new("Missing API key").with_label(
            format!("{} not set and no api_key in config", env_var),
            nu_protocol::Span::unknown(),
        )
    })
}

/// Call an LLM with the given prompt and configuration.
///
/// Creates the appropriate provider client based on config.provider,
/// sends the prompt, and returns the response with usage statistics.
///
/// Supports: openai, anthropic, ollama (custom providers via provider_impl)
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
pub async fn call_llm(
    config: &Config,
    prompt: &str,
    tools: Vec<rig::completion::ToolDefinition>,
) -> Result<LlmResponse, LabeledError> {
    match route_provider(config) {
        ProviderRoute::OpenAI => call_openai(config, prompt, tools).await,
        ProviderRoute::Anthropic => call_anthropic(config, prompt, tools).await,
        ProviderRoute::Ollama => call_ollama(config, prompt, tools).await,
        ProviderRoute::GitHubCopilot { .. } => call_github_copilot(config, prompt, tools).await,
        ProviderRoute::Unsupported(name) => {
            Err(LabeledError::new(format!("Unsupported provider: {}", name)))
        }
    }
}

async fn call_github_copilot(
    config: &Config,
    prompt: &str,
    tools: Vec<rig::completion::ToolDefinition>,
) -> Result<LlmResponse, LabeledError> {
    let api_key = resolve_api_key(config, "github-copilot")?;

    // Parse backend from provider: "github-copilot/anthropic" -> "anthropic"
    let backend = parse_github_copilot_backend(&config.provider)?;

    // Create the base GitHub Copilot client
    let client = if let Some(url) = &config.base_url {
        github_copilot::Client::builder()
            .api_key(api_key)
            .base_url(url.clone())
            .build()
            .map_err(|e| LabeledError::new(format!("Failed to create client: {}", e)))?
    } else {
        github_copilot::Client::builder()
            .api_key(api_key)
            .build()
            .map_err(|e| LabeledError::new(format!("Failed to create client: {}", e)))?
    };

    use rig::completion::Completion;

    match backend {
        "anthropic" => {
            let model = github_copilot::completion::CompletionModel::<
                github_copilot::AnthropicBackend,
                _,
            >::new(client, &config.model);

            let agent = rig::agent::AgentBuilder::new(model).build();

            let builder = agent
                .completion(prompt, Vec::<rig::completion::Message>::new())
                .await
                .map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))?
                .tools(tools);
            let completion_response = builder
                .send()
                .await
                .map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))?;

            // Extract text and tool calls from choice (OneOrMany<AssistantContent>)
            let mut text_parts = Vec::new();
            let mut tool_calls = Vec::new();

            for content in completion_response.choice {
                match content {
                    rig::completion::AssistantContent::Text(t) => {
                        text_parts.push(t.to_string());
                    }
                    tool_call @ rig::completion::AssistantContent::ToolCall(_) => {
                        tool_calls.push(tool_call);
                    }
                    _ => {
                        // Ignore Reasoning, Image, and any future variants
                    }
                }
            }

            let text = text_parts.join("\n");
            let usage = completion_response.usage.into();

            Ok(LlmResponse {
                text,
                usage,
                tool_calls,
            })
        }
        "openai" => {
            let model = github_copilot::completion::CompletionModel::<
                github_copilot::OpenAIBackend,
                _,
            >::new(client, &config.model);

            let agent = rig::agent::AgentBuilder::new(model).build();

            let builder = agent
                .completion(prompt, Vec::<rig::completion::Message>::new())
                .await
                .map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))?
                .tools(tools);
            let completion_response = builder
                .send()
                .await
                .map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))?;

            // Extract text and tool calls from choice (OneOrMany<AssistantContent>)
            let mut text_parts = Vec::new();
            let mut tool_calls = Vec::new();

            for content in completion_response.choice {
                match content {
                    rig::completion::AssistantContent::Text(t) => {
                        text_parts.push(t.to_string());
                    }
                    tool_call @ rig::completion::AssistantContent::ToolCall(_) => {
                        tool_calls.push(tool_call);
                    }
                    _ => {
                        // Ignore Reasoning, Image, and any future variants
                    }
                }
            }

            let text = text_parts.join("\n");
            let usage = completion_response.usage.into();

            Ok(LlmResponse {
                text,
                usage,
                tool_calls,
            })
        }
        _ => Err(LabeledError::new(format!(
            "Unknown GitHub Copilot backend: {}",
            backend
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

async fn call_openai(
    config: &Config,
    prompt: &str,
    tools: Vec<rig::completion::ToolDefinition>,
) -> Result<LlmResponse, LabeledError> {
    let key = resolve_api_key(config, &config.provider.clone())?;

    let client = if let Some(base_url) = &config.base_url {
        openai::Client::builder()
            .api_key(key)
            .base_url(base_url.clone())
            .build()
            .map_err(|e| LabeledError::new(format!("Failed to create OpenAI client: {}", e)))?
    } else if config.api_key.is_some() {
        openai::Client::builder()
            .api_key(key)
            .build()
            .map_err(|e| LabeledError::new(format!("Failed to create OpenAI client: {}", e)))?
    } else {
        return Err(LabeledError::new(
            "OpenAI requires an API key via --api-key or OPENAI_API_KEY".to_string(),
        ));
    };

    let model = openai::Client::completion_model(&client, &config.model);
    let agent = rig::agent::AgentBuilder::new(model).build();

    use rig::completion::Completion;
    let builder = agent
        .completion(prompt, Vec::<rig::completion::Message>::new())
        .await
        .map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))?
        .tools(tools);
    let completion_response = builder
        .send()
        .await
        .map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))?;

    // Extract text and tool calls from choice (OneOrMany<AssistantContent>)
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for content in completion_response.choice {
        match content {
            rig::completion::AssistantContent::Text(t) => {
                text_parts.push(t.to_string());
            }
            tool_call @ rig::completion::AssistantContent::ToolCall(_) => {
                tool_calls.push(tool_call);
            }
            _ => {
                // Ignore Reasoning, Image, and any future variants
            }
        }
    }

    let text = text_parts.join("\n");
    let usage = completion_response.usage.into();

    Ok(LlmResponse {
        text,
        usage,
        tool_calls,
    })
}

async fn call_anthropic(
    config: &Config,
    prompt: &str,
    tools: Vec<rig::completion::ToolDefinition>,
) -> Result<LlmResponse, LabeledError> {
    let key = resolve_api_key(config, "anthropic")?;

    let client = if let Some(base_url) = &config.base_url {
        anthropic::Client::builder()
            .api_key(key)
            .base_url(base_url.clone())
            .build()
            .map_err(|e| LabeledError::new(format!("Failed to create Anthropic client: {}", e)))?
    } else if config.api_key.is_some() {
        anthropic::Client::builder()
            .api_key(key)
            .build()
            .map_err(|e| LabeledError::new(format!("Failed to create Anthropic client: {}", e)))?
    } else {
        anthropic::Client::from_env()
    };

    let mut agent_builder = client.agent(&config.model);

    if let Some(max_tokens) = config.max_tokens {
        agent_builder = agent_builder.max_tokens(max_tokens as u64);
    }

    let agent = agent_builder.build();

    use rig::completion::Completion;
    let builder = agent
        .completion(prompt, Vec::<rig::completion::Message>::new())
        .await
        .map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))?
        .tools(tools);
    let completion_response = builder
        .send()
        .await
        .map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))?;

    // Extract text and tool calls from choice (OneOrMany<AssistantContent>)
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for content in completion_response.choice {
        match content {
            rig::completion::AssistantContent::Text(t) => {
                text_parts.push(t.to_string());
            }
            tool_call @ rig::completion::AssistantContent::ToolCall(_) => {
                tool_calls.push(tool_call);
            }
            _ => {
                // Ignore Reasoning, Image, and any future variants
            }
        }
    }

    let text = text_parts.join("\n");
    let usage = completion_response.usage.into();

    Ok(LlmResponse {
        text,
        usage,
        tool_calls,
    })
}

async fn call_ollama(
    config: &Config,
    prompt: &str,
    tools: Vec<rig::completion::ToolDefinition>,
) -> Result<LlmResponse, LabeledError> {
    let base_url = config
        .base_url
        .as_deref()
        .unwrap_or("http://localhost:11434");

    let client = ollama::Client::builder()
        .api_key(rig::client::Nothing)
        .base_url(base_url)
        .build()
        .map_err(|e| LabeledError::new(format!("Failed to create Ollama client: {}", e)))?;

    let agent = client.agent(&config.model).build();

    use rig::completion::Completion;
    let builder = agent
        .completion(prompt, Vec::<rig::completion::Message>::new())
        .await
        .map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))?
        .tools(tools);
    let completion_response = builder
        .send()
        .await
        .map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))?;

    // Extract text and tool calls from choice (OneOrMany<AssistantContent>)
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for content in completion_response.choice {
        match content {
            rig::completion::AssistantContent::Text(t) => {
                text_parts.push(t.to_string());
            }
            tool_call @ rig::completion::AssistantContent::ToolCall(_) => {
                tool_calls.push(tool_call);
            }
            _ => {
                // Ignore Reasoning, Image, and any future variants
            }
        }
    }

    let text = text_parts.join("\n");
    let usage = completion_response.usage.into();

    Ok(LlmResponse {
        text,
        usage,
        tool_calls,
    })
}

#[cfg(test)]
mod tests;
