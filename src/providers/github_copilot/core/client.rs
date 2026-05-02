//! GitHub Copilot client implementation
//!
//! Provides Provider, ProviderBuilder, and Capabilities trait implementations
//! for the GitHub Copilot API.
//!
//! # Extension Model
//!
//! GitHub Copilot routes to concrete provider implementations selected once in
//! `model_factory`. This client module provides a single extension type used by
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

impl Provider for GitHubCopilotExt {
    type Builder = GitHubCopilotExtBuilder;
    const VERIFY_PATH: &'static str = "/models";
}

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
pub trait ClientExt {
    fn agent_from_config(
        provider_string: &str,
        model: &str,
        api_key: Option<String>,
        base_url: Option<String>,
    ) -> Result<
        crate::providers::github_copilot::model::Agent,
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
        crate::providers::github_copilot::model::Agent,
        crate::providers::github_copilot::Error,
    > {
        crate::providers::github_copilot::model::agent_from_config(
            provider_string,
            model,
            api_key,
            base_url,
        )
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod client_tests;
