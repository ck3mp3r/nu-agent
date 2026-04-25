//! GitHub Copilot client implementation
//!
//! Provides Provider, ProviderBuilder, and Capabilities trait implementations
//! for the GitHub Copilot API.
//!
//! # Backend-Specific Extensions
//!
//! GitHub Copilot proxies to multiple backends (Anthropic, OpenAI), each requiring
//! different intent headers. This module provides separate extension types per backend:
//! - `GitHubCopilotAnthropicExt` - For Claude models (conversation-panel intent)
//! - `GitHubCopilotOpenAIExt` - For GPT models (conversation-agent intent)

use rig::client::{self, Capabilities, Capable, Nothing, Provider, ProviderBuilder};

/// Zero-sized marker type for GitHub Copilot extension (legacy, kept for backward compatibility)
#[derive(Debug, Default, Clone, Copy)]
pub struct GitHubCopilotExt;

/// Zero-sized marker type for GitHub Copilot Anthropic backend
#[derive(Debug, Default, Clone, Copy)]
pub struct GitHubCopilotAnthropicExt;

/// Zero-sized marker type for GitHub Copilot OpenAI backend
#[derive(Debug, Default, Clone, Copy)]
pub struct GitHubCopilotOpenAIExt;

/// Builder for GitHub Copilot extension (legacy)
#[derive(Debug, Default, Clone, Copy)]
pub struct GitHubCopilotExtBuilder;

/// Builder for GitHub Copilot Anthropic extension
#[derive(Debug, Default, Clone, Copy)]
pub struct GitHubCopilotAnthropicExtBuilder;

/// Builder for GitHub Copilot OpenAI extension
#[derive(Debug, Default, Clone, Copy)]
pub struct GitHubCopilotOpenAIExtBuilder;

/// Type alias for GitHub Copilot client (legacy, defaults to OpenAI backend)
pub type Client<H = reqwest::Client> = client::Client<GitHubCopilotExt, H>;

/// Type alias for GitHub Copilot Anthropic client
pub type AnthropicClient<H = reqwest::Client> = client::Client<GitHubCopilotAnthropicExt, H>;

/// Type alias for GitHub Copilot OpenAI client
pub type OpenAIClient<H = reqwest::Client> = client::Client<GitHubCopilotOpenAIExt, H>;

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
    type Completion = Capable<super::completion::CompletionModel<super::OpenAIBackend, H>>;
    type Embeddings = Nothing;
    type Transcription = Nothing;
    type ModelListing = Nothing;
}

// ============================================================================
// GitHubCopilotAnthropicExt
// ============================================================================

impl Provider for GitHubCopilotAnthropicExt {
    type Builder = GitHubCopilotAnthropicExtBuilder;
    const VERIFY_PATH: &'static str = "/models";
}

impl ProviderBuilder for GitHubCopilotAnthropicExtBuilder {
    type Extension<H>
        = GitHubCopilotAnthropicExt
    where
        H: rig::http_client::HttpClientExt;
    type ApiKey = client::BearerAuth;

    const BASE_URL: &'static str = "https://api.githubcopilot.com";

    fn build<H>(
        _builder: &client::ClientBuilder<Self, client::BearerAuth, H>,
    ) -> rig::http_client::Result<Self::Extension<H>>
    where
        H: rig::http_client::HttpClientExt,
    {
        Ok(GitHubCopilotAnthropicExt)
    }
}

impl<H> Capabilities<H> for GitHubCopilotAnthropicExt
where
    H: rig::http_client::HttpClientExt,
{
    type Completion = Capable<super::completion::CompletionModel<super::AnthropicBackend, H>>;
    type Embeddings = Nothing;
    type Transcription = Nothing;
    type ModelListing = Nothing;
}

// ============================================================================
// GitHubCopilotOpenAIExt
// ============================================================================

impl Provider for GitHubCopilotOpenAIExt {
    type Builder = GitHubCopilotOpenAIExtBuilder;
    const VERIFY_PATH: &'static str = "/models";
}

impl ProviderBuilder for GitHubCopilotOpenAIExtBuilder {
    type Extension<H>
        = GitHubCopilotOpenAIExt
    where
        H: rig::http_client::HttpClientExt;
    type ApiKey = client::BearerAuth;

    const BASE_URL: &'static str = "https://api.githubcopilot.com";

    fn build<H>(
        _builder: &client::ClientBuilder<Self, client::BearerAuth, H>,
    ) -> rig::http_client::Result<Self::Extension<H>>
    where
        H: rig::http_client::HttpClientExt,
    {
        Ok(GitHubCopilotOpenAIExt)
    }
}

impl<H> Capabilities<H> for GitHubCopilotOpenAIExt
where
    H: rig::http_client::HttpClientExt,
{
    type Completion = Capable<super::completion::CompletionModel<super::OpenAIBackend, H>>;
    type Embeddings = Nothing;
    type Transcription = Nothing;
    type ModelListing = Nothing;
}

#[cfg(test)]
#[path = "client_test.rs"]
mod tests;
