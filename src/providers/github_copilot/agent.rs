//! Agent factory for GitHub Copilot
//!
//! Provides a unified agent creation interface that handles backend selection
//! and configuration.

use super::Error;

/// GitHub Copilot Agent wrapper for concrete provider variants
///
/// This enum wraps agents for concrete provider variants to provide a unified
/// interface for agent creation from configuration.
pub enum Agent<H = reqwest::Client>
where
    H: rig::http_client::HttpClientExt + Default + std::fmt::Debug + Clone + 'static,
{
    /// Anthropic backend (Claude models)
    Anthropic(
        rig::agent::Agent<
            super::completion::CompletionModel<super::providers::AnthropicProvider, H>,
        >,
    ),
    /// OpenAI 4x backend (GPT-4* and non-5 OpenAI models)
    OpenAI4x(
        rig::agent::Agent<
            super::completion::CompletionModel<super::providers::OpenAI4xProvider, H>,
        >,
    ),
    /// OpenAI 5x backend (GPT-5* models)
    OpenAI5x(
        rig::agent::Agent<
            super::completion::CompletionModel<super::providers::OpenAI5xProvider, H>,
        >,
    ),
}

impl Agent {
    /// Create agent from config - handles backend parsing and API key resolution
    ///
    /// This factory method encapsulates all GitHub Copilot agent creation logic,
    /// including:
    /// - Backend parsing from model string (e.g., "anthropic/claude-sonnet-4.5")
    /// - Concrete provider variant selection
    /// - Model extraction from format "backend/model"
    /// - API key resolution (config → GITHUB_TOKEN env var → error)
    /// - Client construction with optional base_url
    /// - Agent creation with appropriate backend
    ///
    /// # Arguments
    ///
    /// * `provider_string` - Must be exactly "github-copilot"
    /// * `model` - Model identifier in format "backend/model"
    ///   (e.g., "anthropic/claude-sonnet-4.5", "openai/gpt-4o", "openai/gpt-5.3-codex")
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
    /// - Backend is not one of: "anthropic", "openai"
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
        super::factory::agent_from_config(provider_string, model, api_key, base_url)
    }
}

#[cfg(test)]
#[path = "agent_test.rs"]
mod tests;
