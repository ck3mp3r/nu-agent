//! GitHub Copilot client implementation
//!
//! Provides Provider, ProviderBuilder, and Capabilities trait implementations
//! for the GitHub Copilot API.
//!
//! # Extension Model
//!
//! GitHub Copilot routes to concrete provider implementations selected once in
//! `model::factory`. This client module provides a single extension type used by
//! all concrete providers. Endpoint and payload semantics are not owned here.

use rig::client::{self, Capabilities, Capable, Nothing, Provider, ProviderBuilder};

/// Zero-sized marker type for GitHub Copilot extension (legacy, kept for backward compatibility)
#[derive(Debug, Default, Clone, Copy)]
pub struct GitHubCopilotExt;

/// Builder for GitHub Copilot extension (legacy)
#[derive(Debug, Default, Clone, Copy)]
pub struct GitHubCopilotExtBuilder;

/// Type alias for GitHub Copilot client (legacy, defaults to OpenAI backend)
pub type Client<H = reqwest::Client> = client::Client<GitHubCopilotExt, H>;

/// Type alias for GitHub Copilot client builder (legacy)
pub type ClientBuilder<H = reqwest::Client> =
    client::ClientBuilder<GitHubCopilotExtBuilder, client::BearerAuth, H>;

// ============================================================================
// Legacy GitHubCopilotExt (backward compatibility, defaults to OpenAI)
// ============================================================================

// Implement Provider trait
impl Provider for GitHubCopilotExt {
    type Builder = GitHubCopilotExtBuilder;
    const VERIFY_PATH: &'static str = "/models";
}

// Implement ProviderBuilder trait
impl ProviderBuilder for GitHubCopilotExtBuilder {
    type Extension<H>
        = GitHubCopilotExt
    where
        H: rig::http_client::HttpClientExt;
    type ApiKey = client::BearerAuth;

    const BASE_URL: &'static str = "https://api.githubcopilot.com";

    fn build<H>(_builder: &ClientBuilder<H>) -> rig::http_client::Result<Self::Extension<H>>
    where
        H: rig::http_client::HttpClientExt,
    {
        Ok(GitHubCopilotExt)
    }
}

// Implement Capabilities trait (defaults to OpenAI backend)
impl<H> Capabilities<H> for GitHubCopilotExt
where
    H: rig::http_client::HttpClientExt,
{
    type Completion = Capable<
        crate::providers::github_copilot::completion::CompletionModel<
            crate::providers::github_copilot::providers::OpenAI4xProvider,
            H,
        >,
    >;
    type Embeddings = Nothing;
    type Transcription = Nothing;
    type ModelListing = Nothing;
}

// ============================================================================
// Factory Methods Extension Trait
// ============================================================================

/// Extension trait for Client to add factory methods
///
/// This trait provides factory methods for creating agents from configuration.
/// It's implemented for Client type alias to work around Rust's orphan rules.
pub trait ClientExt {
    /// Create agent from config - handles backend parsing and API key resolution
    ///
    /// This factory method encapsulates all GitHub Copilot agent creation logic,
    /// including:
    /// - Backend parsing from model string (e.g., "anthropic/claude-sonnet-4.5")
    /// - Model extraction from format "backend/model"
    /// - One-time concrete provider selection in factory
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
    /// Returns an Agent wrapper that implements Completion trait.
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
    /// use nu_plugin_agent::providers::github_copilot::{Client, ClientExt};
    ///
    /// # fn example() -> Result<(), nu_plugin_agent::providers::github_copilot::Error> {
    /// // Create OpenAI backend agent with explicit API key
    /// let agent = Client::agent_from_config(
    ///     "github-copilot",
    ///     "openai/gpt-4o",
    ///     Some("your-token".to_string()),
    ///     None,
    /// )?;
    ///
    /// // Create Anthropic backend agent using GITHUB_TOKEN env var
    /// unsafe { std::env::set_var("GITHUB_TOKEN", "your-token"); }
    /// let agent = Client::agent_from_config(
    ///     "github-copilot",
    ///     "anthropic/claude-sonnet-4.5",
    ///     None,
    ///     None,
    /// )?;
    /// # Ok(())
    /// # }
    /// ```
    fn agent_from_config(
        provider_string: &str,
        model: &str,
        api_key: Option<String>,
        base_url: Option<String>,
    ) -> Result<
        crate::providers::github_copilot::model::factory::Agent,
        crate::providers::github_copilot::Error,
    >;
}

impl ClientExt for Client {
    fn agent_from_config(
        provider_string: &str,
        model: &str,
        api_key: Option<String>,
        base_url: Option<String>,
    ) -> Result<
        crate::providers::github_copilot::model::factory::Agent,
        crate::providers::github_copilot::Error,
    > {
        crate::providers::github_copilot::model::factory::agent_from_config(
            provider_string,
            model,
            api_key,
            base_url,
        )
    }
}

#[cfg(test)]
#[path = "test.rs"]
mod test;
