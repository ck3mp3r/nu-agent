use crate::config::Config;
use crate::providers::github_copilot;
use nu_protocol::{LabeledError, Span, Value};
use rig::client::{CompletionClient, ProviderClient};
use rig::providers::{anthropic, ollama, openai};

/// Call an LLM with the given prompt and configuration.
///
/// Creates the appropriate provider client based on config.provider,
/// sends the prompt, and returns the response.
///
/// Supports: openai, anthropic, ollama (custom providers via provider_impl)
///
/// # Arguments
/// * `config` - Configuration with provider, model, and auth details
/// * `prompt` - The prompt to send to the LLM
///
/// # Returns
/// The LLM response as a string, or error if the call fails
///
/// # Errors
/// - Missing API key when required (checks env var if config.api_key is None)
/// - Unsupported provider
/// - Invalid configuration
/// - API errors (network, rate limits, etc.)
pub async fn call_llm(config: &Config, prompt: &str) -> Result<String, LabeledError> {
    // Check if provider starts with "github-copilot/" (3-part format: github-copilot/backend/model)
    if config.provider.starts_with("github-copilot/") {
        call_github_copilot(config, prompt).await
    } else {
        // Legacy routing: use provider_impl or provider name (2-part format: provider/model)
        let provider_name = config.provider_impl.as_deref().unwrap_or(&config.provider);
        
        match provider_name {
            "github-copilot" | "openai" if config.base_url.is_some() && config.base_url.as_ref().unwrap().contains("githubcopilot") => {
                call_github_copilot(config, prompt).await
            }
            "openai" => call_openai(config, prompt).await,
            "anthropic" => call_anthropic(config, prompt).await,
            "ollama" => call_ollama(config, prompt).await,
            _ => Err(LabeledError::new(format!(
                "Unsupported provider: {}",
                provider_name
            ))),
        }
    }
}

async fn call_github_copilot(config: &Config, prompt: &str) -> Result<String, LabeledError> {
    let api_key = if let Some(ref key) = config.api_key {
            key.clone()
        } else {
            std::env::var("GITHUB_TOKEN")
                .map_err(|_| {
                LabeledError::new("Missing API key").with_label(
                    "GITHUB_TOKEN not set and no api_key in config",
                    nu_protocol::Span::unknown(),
                )
            })?
    };
    
    // Parse backend from provider: "github-copilot/anthropic" -> "anthropic"
    let backend = config.provider.strip_prefix("github-copilot/")
        .ok_or_else(|| LabeledError::new("Invalid GitHub Copilot provider format"))?;
    
    // Create the base GitHub Copilot client (legacy Client type)
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
    
    // Import Prompt trait for .prompt() method
    use rig::completion::Prompt;
    
    // Create CompletionModel with appropriate backend and wrap in an agent
    match backend {
        "anthropic" => {
            let model = github_copilot::completion::CompletionModel::<github_copilot::AnthropicBackend, _>::new(client, &config.model);
            let agent = rig::agent::AgentBuilder::new(model).build();
            agent.prompt(prompt).await.map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))
        }
        "openai" => {
            let model = github_copilot::completion::CompletionModel::<github_copilot::OpenAIBackend, _>::new(client, &config.model);
            let agent = rig::agent::AgentBuilder::new(model).build();
            agent.prompt(prompt).await.map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))
        }
        _ => Err(LabeledError::new(format!("Unknown GitHub Copilot backend: {}", backend))),
    }
}

/// Format LLM response as a Nushell Value::Record.
///
/// Creates a record with the following fields:
/// - `response`: The LLM response text (String)
/// - `model`: The model used (String)
/// - `provider`: The provider used (String)
/// - `timestamp`: ISO8601 timestamp of when the response was created (String)
///
/// # Arguments
/// * `response` - The LLM response text
/// * `config` - The configuration used for the request
/// * `span` - The span for the Value
///
/// # Returns
/// A Value::Record containing the response and metadata
pub fn format_response(response: &str, config: &Config, span: Span) -> Value {
    use chrono::Utc;

    let timestamp = Utc::now().to_rfc3339();

    Value::record(
        vec![
            ("response".to_string(), Value::string(response, span)),
            ("model".to_string(), Value::string(&config.model, span)),
            (
                "provider".to_string(),
                Value::string(&config.provider, span),
            ),
            ("timestamp".to_string(), Value::string(timestamp, span)),
        ]
        .into_iter()
        .collect(),
        span,
    )
}

/// Helper function to check if API key is available (from config or env)
fn get_api_key(config: &Config, provider: &str) -> Result<Option<String>, LabeledError> {
    if let Some(ref api_key) = config.api_key {
        return Ok(Some(api_key.clone()));
    }

    // Special case: github-copilot uses GITHUB_TOKEN
    let env_var = if provider == "github-copilot" {
        "GITHUB_TOKEN".to_string()
    } else {
        format!("{}_API_KEY", provider.to_uppercase())
    };
    
    match std::env::var(&env_var) {
        Ok(key) => Ok(Some(key)),
        Err(_) => Ok(None),
    }
}

async fn call_openai(config: &Config, prompt: &str) -> Result<String, LabeledError> {
    // Check for API key
    let api_key = get_api_key(config, &config.provider)?;

    let key = api_key.ok_or_else(|| {
        LabeledError::new("Missing API key").with_label(
            "OPENAI_API_KEY not set and no api_key in config",
            nu_protocol::Span::unknown(),
        )
    })?;

    // Create client
    let client = if let Some(base_url) = &config.base_url {
        // Use builder for base_url override
        openai::Client::builder()
            .api_key(key)
            .base_url(base_url.clone())
            .build()
            .map_err(|e| LabeledError::new(format!("Failed to create OpenAI client: {}", e)))?
    } else if config.api_key.is_some() {
        // Use builder if explicit API key is provided in config
        openai::Client::builder()
            .api_key(key)
            .build()
            .map_err(|e| LabeledError::new(format!("Failed to create OpenAI client: {}", e)))?
    } else {
        // Use from_env() only when using env var (no config overrides)
        openai::Client::from_env()
    };

    // Create agent with model
    let agent = client.agent(&config.model).build();

    // Call LLM
    use rig::completion::Prompt;
    agent
        .prompt(prompt)
        .await
        .map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))
}

async fn call_anthropic(config: &Config, prompt: &str) -> Result<String, LabeledError> {
    // Check for API key
    let api_key = get_api_key(config, "anthropic")?;

    if api_key.is_none() {
        return Err(LabeledError::new("Missing API key").with_label(
            "ANTHROPIC_API_KEY not set and no api_key in config",
            nu_protocol::Span::unknown(),
        ));
    }

    // Create client - Anthropic doesn't support base_url override in the same way
    let client = anthropic::Client::from_env();

    // Create agent with model
    let agent = client.agent(&config.model).build();

    // Call LLM
    use rig::completion::Prompt;
    agent
        .prompt(prompt)
        .await
        .map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))
}

async fn call_ollama(config: &Config, prompt: &str) -> Result<String, LabeledError> {
    // Ollama doesn't require an API key
    let base_url = config
        .base_url
        .as_deref()
        .unwrap_or("http://localhost:11434");

    // Create client with base URL
    let client = ollama::Client::builder()
        .api_key(rig::client::Nothing)
        .base_url(base_url)
        .build()
        .map_err(|e| LabeledError::new(format!("Failed to create Ollama client: {}", e)))?;

    // Create agent with model
    let agent = client.agent(&config.model).build();

    // Call LLM
    use rig::completion::Prompt;
    agent
        .prompt(prompt)
        .await
        .map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))
}


#[cfg(test)]
#[path = "llm_test.rs"]
mod tests;
