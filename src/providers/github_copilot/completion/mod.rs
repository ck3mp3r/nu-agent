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
use rig::http_client::{self, HeaderValue, HttpClientExt};
use rig::wasm_compat::{WasmCompatSend, WasmCompatSync};
use serde::{Deserialize, Serialize};
use serde_json;

/// GitHub Copilot error response structure
#[derive(Debug, Deserialize)]
struct GitHubCopilotError {
    #[serde(default)]
    error: Option<ErrorDetail>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorDetail {
    message: String,
    #[serde(default)]
    #[allow(dead_code)] // May be used for debugging in future
    code: Option<String>,
}

/// GitHub Copilot Choice (more lenient than OpenAI's - index is optional)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GitHubCopilotChoice {
    #[serde(default)]
    pub index: Option<usize>,  // Optional - Claude responses omit this
    pub message: rig::providers::openai::completion::Message,
    #[serde(default)]
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub logprobs: Option<serde_json::Value>,
}

impl From<GitHubCopilotChoice> for rig::providers::openai::completion::Choice {
    fn from(choice: GitHubCopilotChoice) -> Self {
        Self {
            index: choice.index.unwrap_or(0),  // Default to 0 if missing
            message: choice.message,
            finish_reason: choice.finish_reason.unwrap_or_else(|| "stop".to_string()),
            logprobs: None,  // Ignore logprobs for now
        }
    }
}

/// GitHub Copilot completion response (more lenient than OpenAI's)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GitHubCopilotCompletionResponse {
    pub id: String,
    #[serde(default)]
    pub object: Option<String>,  // Optional - GitHub Copilot sometimes omits this
    #[serde(default)]
    pub created: Option<u64>,    // Optional - GitHub Copilot sometimes omits this
    pub model: String,
    pub choices: Vec<GitHubCopilotChoice>,
    #[serde(default)]
    pub usage: Option<rig::providers::openai::completion::Usage>,
    #[serde(default)]
    pub system_fingerprint: Option<String>,
}

impl From<GitHubCopilotCompletionResponse> for rig::providers::openai::completion::CompletionResponse {
    fn from(response: GitHubCopilotCompletionResponse) -> Self {
        Self {
            id: response.id,
            object: response.object.unwrap_or_else(|| "chat.completion".to_string()),
            created: response.created.unwrap_or(0),  // Default to 0 if missing
            model: response.model,
            choices: response.choices.into_iter().map(|c| c.into()).collect(),
            usage: response.usage,
            system_fingerprint: response.system_fingerprint,
        }
    }
}

/// Completion model for GitHub Copilot
///
/// GitHub Copilot uses an OpenAI-compatible API but requires specific headers.
/// The model is generic over:
/// - B: GitHubCopilotBackend - determines the correct intent header
/// - H: HttpClientExt - the HTTP client implementation
#[derive(Clone)]
pub struct CompletionModel<B = super::OpenAIBackend, H = reqwest::Client>
where
    B: super::GitHubCopilotBackend,
    H: HttpClientExt,
{
    pub(crate) backend: B,
    pub(crate) client: rig::client::Client<super::GitHubCopilotExt, H>,
    pub model: String,
}

impl<B, H> CompletionModel<B, H>
where
    B: super::GitHubCopilotBackend,
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
            backend: B::default(),
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
            backend: B::default(),
            client,
            model: model.into(),
        }
    }
}

impl<B, H> rig::completion::request::CompletionModel for CompletionModel<B, H>
where
    B: super::GitHubCopilotBackend,
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
        // Reuse OpenAI's request conversion since GitHub Copilot is API-compatible
        let request = rig::providers::openai::completion::CompletionRequest::try_from(
            rig::providers::openai::completion::OpenAIRequestParams {
                model: self.model.to_owned(),
                request: completion_request,
                strict_tools: false,
                tool_result_array_content: false,
            },
        )?;

        let body = serde_json::to_vec(&request)?;

        // Build request with GitHub Copilot-specific headers
        let mut req = self.client.post("/chat/completions")?;

        // Get the correct intent header from the backend (determined at creation time)
        let intent = self.backend.intent_header();

        // Add GitHub-specific headers BEFORE setting body
        if let Some(headers) = req.headers_mut() {
            headers.insert(
                "User-Agent",
                HeaderValue::from_static("GitHubCopilotChat/0.1"),
            );
            headers.insert(
                "Copilot-Integration-Id",
                HeaderValue::from_static("vscode-chat"),
            );
            headers.insert(
                "editor-version",
                HeaderValue::from_static("vscode/1.85.0"),
            );
            headers.insert(
                "editor-plugin-version",
                HeaderValue::from_static("copilot-chat/0.11.1"),
            );
            headers.insert(
                "openai-organization",
                HeaderValue::from_static("github-copilot"),
            );
            headers.insert(
                "openai-intent",
                HeaderValue::from_static(intent),
            );
        }

        let req = req
            .body(body)
            .map_err(|e| CompletionError::HttpError(e.into()))?;

        let response = self.client.send(req).await?;

        let status = response.status();
        let text = http_client::text(response).await?;

        // Check if response is HTML (common error case)
        if text.trim_start().starts_with("<!DOCTYPE") || text.trim_start().starts_with("<html") {
            return Err(CompletionError::ProviderError(format!(
                "Received HTML response (likely authentication or endpoint error). HTTP status: {}. Check your GitHub token and base URL.",
                status
            )));
        }

        if status.is_success() {
            // Try to parse as GitHub Copilot response (more lenient than OpenAI)
            match serde_json::from_str::<GitHubCopilotCompletionResponse>(&text) {
                Ok(response) => {
                    // Convert to OpenAI format for rig compatibility
                    let openai_response: rig::providers::openai::completion::CompletionResponse = response.into();
                    openai_response.try_into()
                }
                Err(parse_err) => {
                    // If parsing failed, try to parse as error response
                    match serde_json::from_str::<GitHubCopilotError>(&text) {
                        Ok(err_response) => {
                            let error_msg = err_response
                                .error
                                .map(|e| e.message)
                                .or(err_response.message)
                                .unwrap_or_else(|| "Unknown error".to_string());
                            Err(CompletionError::ProviderError(error_msg))
                        }
                        Err(_) => {
                            // If both failed, provide helpful error with snippet
                            let snippet = if text.len() > 200 {
                                format!("{}...", &text[..200])
                            } else {
                                text.clone()
                            };
                            Err(CompletionError::ProviderError(format!(
                                "Failed to parse GitHub Copilot response. Parse error: {}. Response snippet: {}",
                                parse_err, snippet
                            )))
                        }
                    }
                }
            }
        } else {
            // Non-success status - try to extract error message
            match serde_json::from_str::<GitHubCopilotError>(&text) {
                Ok(err_response) => {
                    let error_msg = err_response
                        .error
                        .map(|e| e.message)
                        .or(err_response.message)
                        .unwrap_or_else(|| text.clone());
                    Err(CompletionError::ProviderError(format!(
                        "HTTP {}: {}",
                        status, error_msg
                    )))
                }
                Err(_) => {
                    // Return raw text for non-JSON error responses
                    Err(CompletionError::ProviderError(format!(
                        "HTTP {}: {}",
                        status, text
                    )))
                }
            }
        }
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
#[path = "completion_test.rs"]
mod tests;
