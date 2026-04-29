//! GitHub Copilot completion model implementation
//!
//! This module provides the completion model for GitHub Copilot, implementing
//! rig's `CompletionModel` trait. The implementation is forked from OpenAI's
//! provider to add GitHub-specific HTTP headers required for API compatibility.
//!
//! # GitHub-Specific Headers
//!
//! GitHub Copilot requires two critical headers:
//! - `openai-organization: github-copilot` - Identifies the organization
//! - `openai-intent: conversation-panel` - Specifies the use case
//!
//! These headers are automatically added to all completion requests.
//!
//! # Usage
//!
//! The `CompletionModel` is typically accessed via the client's agent:
//!
//! ```no_run
//! use nu_plugin_agent::providers::github_copilot;
//! use rig::client::CompletionClient;
//! use rig::completion::Prompt;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = github_copilot::Client::builder()
//!     .api_key("token")
//!     .build()?;
//!
//! let agent = client.agent("gpt-4o").build();
//! let response = agent.prompt("Hello!").await?;
//! # Ok(())
//! # }
//! ```

use rig::completion::request::{CompletionError, CompletionRequest as CoreCompletionRequest};
use rig::http_client::HttpClientExt;
use rig::wasm_compat::{WasmCompatSend, WasmCompatSync};

/// Completion model for GitHub Copilot
///
/// GitHub Copilot uses an OpenAI-compatible API but requires specific headers.
/// The model is generic over:
/// - P: concrete provider implementation selected once by factory
/// - H: HttpClientExt - the HTTP client implementation
#[derive(Clone)]
pub struct CompletionModel<P = super::providers::OpenAI4xProvider, H = reqwest::Client>
where
    P: super::providers::contract::GitHubCopilotProvider,
    H: HttpClientExt,
{
    pub(crate) _provider: std::marker::PhantomData<P>,
    pub(crate) client: rig::client::Client<super::GitHubCopilotExt, H>,
    pub model: String,
}

impl<P, H> CompletionModel<P, H>
where
    P: super::providers::contract::GitHubCopilotProvider,
    H: HttpClientExt + Default + std::fmt::Debug + Clone + 'static,
{
    /// Create a new completion model
    ///
    /// # Arguments
    ///
    /// * `client` - The GitHub Copilot client
    /// * `model` - Model identifier (e.g., "gpt-4o", "claude-sonnet-4.5")
    pub fn new(
        client: rig::client::Client<super::GitHubCopilotExt, H>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            _provider: std::marker::PhantomData,
            client,
            model: model.into(),
        }
    }

    /// Create a completion model with the specified model name
    ///
    /// # Arguments
    ///
    /// * `client` - The GitHub Copilot client
    /// * `model` - Model identifier string
    pub fn with_model(
        client: rig::client::Client<super::GitHubCopilotExt, H>,
        model: &str,
    ) -> Self {
        Self {
            _provider: std::marker::PhantomData,
            client,
            model: model.into(),
        }
    }
}

impl<P, H> rig::completion::request::CompletionModel for CompletionModel<P, H>
where
    P: super::providers::contract::GitHubCopilotProvider,
    H: HttpClientExt
        + Default
        + std::fmt::Debug
        + Clone
        + WasmCompatSend
        + WasmCompatSync
        + 'static,
{
    type Response = rig::providers::openai::completion::CompletionResponse;
    type StreamingResponse =
        rig::providers::openai::completion::streaming::StreamingCompletionResponse;
    type Client = rig::client::Client<super::GitHubCopilotExt, H>;

    fn make(client: &Self::Client, model: impl Into<String>) -> Self {
        Self::new(client.clone(), model)
    }

    async fn completion(
        &self,
        completion_request: CoreCompletionRequest,
    ) -> Result<rig::completion::CompletionResponse<Self::Response>, CompletionError> {
        P::execute(&self.client, &self.model, completion_request).await
    }

    async fn stream(
        &self,
        _request: CoreCompletionRequest,
    ) -> Result<rig::streaming::StreamingCompletionResponse<Self::StreamingResponse>, CompletionError>
    {
        // TODO: Implement streaming support
        // For now, return error
        Err(CompletionError::ProviderError(
            "Streaming not yet implemented for GitHub Copilot".to_string(),
        ))
    }
}

#[cfg(test)]
#[path = "test.rs"]
mod test;
