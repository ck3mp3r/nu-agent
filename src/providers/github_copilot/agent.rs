//! Agent factory for GitHub Copilot
//!
//! Provides a unified agent creation interface that handles backend selection
//! and configuration.

use super::{AnthropicBackend, Error, OpenAIBackend};

/// GitHub Copilot Agent wrapper supporting multiple backends
///
/// This enum wraps agents for different backends (Anthropic, OpenAI) to provide
/// a unified interface for agent creation from configuration.
pub enum Agent<H = reqwest::Client>
where
    H: rig::http_client::HttpClientExt + Default + std::fmt::Debug + Clone + 'static,
{
    /// Anthropic backend (Claude models)
    Anthropic(rig::agent::Agent<super::completion::CompletionModel<AnthropicBackend, H>>),
    /// OpenAI backend (GPT models)
    OpenAI(rig::agent::Agent<super::completion::CompletionModel<OpenAIBackend, H>>),
}

impl Agent {
    /// Create agent from config - handles backend parsing and API key resolution
    ///
    /// This factory method encapsulates all GitHub Copilot agent creation logic,
    /// including:
    /// - Backend parsing from model string (e.g., "anthropic/claude-sonnet-4.5")
    /// - Model extraction from format "backend/model"
    /// - API key resolution (config → GITHUB_TOKEN env var → error)
    /// - Client construction with optional base_url
    /// - Agent creation with appropriate backend
    ///
    /// # Arguments
    ///
    /// * `provider_string` - Must be exactly "github-copilot"
    /// * `model` - Model identifier in format "backend/model"
    ///   (e.g., "anthropic/claude-sonnet-4.5", "openai/gpt-4o")
    /// * `api_key` - Optional API key (if None, reads from GITHUB_TOKEN env var)
    /// * `base_url` - Optional base URL override (useful for testing)
    ///
    /// # Returns
    ///
    /// Returns an agent wrapper ready for making completion requests.
    ///
    /// # Errors
    ///
    /// Returns `Error` if:
    /// - Provider string is not exactly "github-copilot"
    /// - Model format is invalid (missing "/backend" separator)
    /// - Backend is not "anthropic" or "openai"
    /// - API key not provided and GITHUB_TOKEN env var not set
    /// - Client creation fails (network/configuration issues)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use nu_plugin_agent::providers::github_copilot;
    ///
    /// # fn example() -> Result<(), nu_plugin_agent::providers::github_copilot::Error> {
    /// // Create OpenAI backend agent with explicit API key
    /// let agent = github_copilot::Agent::from_config(
    ///     "github-copilot",
    ///     "openai/gpt-4o",
    ///     Some("your-token".to_string()),
    ///     None,
    /// )?;
    ///
    /// // Create Anthropic backend agent using GITHUB_TOKEN env var
    /// unsafe { std::env::set_var("GITHUB_TOKEN", "your-token"); }
    /// let agent = github_copilot::Agent::from_config(
    ///     "github-copilot",
    ///     "anthropic/claude-sonnet-4.5",
    ///     None,
    ///     None,
    /// )?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_config(
        provider_string: &str,
        model: &str,
        api_key: Option<String>,
        base_url: Option<String>,
    ) -> Result<Self, Error> {
        // Verify provider is exactly "github-copilot"
        if provider_string != "github-copilot" {
            return Err(Error::InvalidProviderFormat(provider_string.to_string()));
        }

        // Parse backend from model: "anthropic/claude-sonnet-4.5" -> ("anthropic", "claude-sonnet-4.5")
        let (backend, model_name) = model
            .split_once('/')
            .ok_or_else(|| Error::InvalidModelFormat(model.to_string()))?;

        // Resolve API key (from config or GITHUB_TOKEN env var)
        let key = api_key
            .or_else(|| std::env::var("GITHUB_TOKEN").ok())
            .ok_or(Error::MissingApiKey)?;

        // Build client
        let client = if let Some(url) = base_url {
            super::Client::builder()
                .api_key(key)
                .base_url(url)
                .build()?
        } else {
            super::Client::builder().api_key(key).build()?
        };

        // Create agent using model_name (not full model string)
        let agent = match backend {
            "anthropic" => {
                let model = super::completion::CompletionModel::<AnthropicBackend, _>::new(
                    client, model_name,
                );
                Agent::Anthropic(rig::agent::AgentBuilder::new(model).build())
            }
            "openai" => {
                let model =
                    super::completion::CompletionModel::<OpenAIBackend, _>::new(client, model_name);
                Agent::OpenAI(rig::agent::AgentBuilder::new(model).build())
            }
            _ => return Err(Error::UnknownBackend(backend.to_string())),
        };

        Ok(agent)
    }
}

#[cfg(test)]
#[path = "agent_test.rs"]
mod tests;
