use crate::providers::github_copilot::providers::contract::{
    CopilotCompletion, CopilotResponse, GitHubCopilotProvider,
};
use rig::completion::request::{CompletionError, CompletionRequest as CoreCompletionRequest};
use rig::http_client::{self, HeaderValue, HttpClientExt};
use serde::{Deserialize, Serialize};

// Mirror rig's ApiResponse envelope structure for response parsing
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ApiResponse<T> {
    Ok(T),
    Err(ErrorEnvelope),
}

#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    error: ApiErrorResponse,
}

#[derive(Debug, Deserialize)]
struct ApiErrorResponse {
    message: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAI4xProvider;

impl GitHubCopilotProvider for OpenAI4xProvider {
    const NAME: &'static str = "OpenAI4xProvider";
    const ENDPOINT_PATH: &'static str = "/chat/completions";
    const INTENT_HEADER: &'static str = "conversation-agent";

    fn map_request(
        model: &str,
        completion_request: CoreCompletionRequest,
    ) -> Result<Vec<u8>, CompletionError> {
        let request = rig::providers::openai::completion::CompletionRequest::try_from(
            rig::providers::openai::completion::OpenAIRequestParams {
                model: model.to_owned(),
                request: completion_request,
                strict_tools: true,
                tool_result_array_content: false,
            },
        )?;
        serde_json::to_vec(&request).map_err(Into::into)
    }

    fn map_response(text: &str) -> Result<CopilotResponse, CompletionError> {
        match serde_json::from_str::<ApiResponse<GitHubCopilotCompletionResponse>>(text)? {
            ApiResponse::Ok(response) => Ok(response.into()),
            ApiResponse::Err(err) => Err(CompletionError::ProviderError(err.error.message)),
        }
    }

    fn map_error(_status: reqwest::StatusCode, text: &str) -> CompletionError {
        CompletionError::ProviderError(text.to_string())
    }

    #[allow(clippy::manual_async_fn)]
    fn execute<'a, H>(
        client: &'a rig::client::Client<super::super::GitHubCopilotExt, H>,
        model: &'a str,
        completion_request: CoreCompletionRequest,
    ) -> impl std::future::Future<Output = Result<CopilotCompletion, CompletionError>> + Send + 'a
    where
        H: HttpClientExt
            + Default
            + std::fmt::Debug
            + Clone
            + rig::wasm_compat::WasmCompatSend
            + rig::wasm_compat::WasmCompatSync
            + 'static,
    {
        async move {
            let body = Self::map_request(model, completion_request)?;

            let mut req = client.post(Self::ENDPOINT_PATH)?;
            if let Some(headers) = req.headers_mut() {
                headers.insert(
                    "User-Agent",
                    HeaderValue::from_static("GitHubCopilotChat/0.1"),
                );
                headers.insert(
                    "Copilot-Integration-Id",
                    HeaderValue::from_static("vscode-chat"),
                );
                headers.insert("editor-version", HeaderValue::from_static("vscode/1.85.0"));
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
                    HeaderValue::from_static(Self::INTENT_HEADER),
                );
            }

            let req = req
                .body(body)
                .map_err(|e| CompletionError::HttpError(e.into()))?;

            let response = client.send(req).await?;
            let status = response.status();
            let text = http_client::text(response).await?;

            if text.trim_start().starts_with("<!DOCTYPE") || text.trim_start().starts_with("<html")
            {
                return Err(CompletionError::ProviderError(format!(
                    "{} {} received HTML response. HTTP status: {}",
                    Self::NAME,
                    Self::ENDPOINT_PATH,
                    status
                )));
            }

            if status.is_success() {
                let parsed = Self::map_response(&text)?;
                parsed.try_into()
            } else {
                Err(Self::map_error(status, &text))
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GitHubCopilotChoice {
    #[serde(default)]
    pub index: Option<usize>,
    pub message: rig::providers::openai::completion::Message,
    #[serde(default)]
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub logprobs: Option<serde_json::Value>,
}

impl From<GitHubCopilotChoice> for rig::providers::openai::completion::Choice {
    fn from(choice: GitHubCopilotChoice) -> Self {
        Self {
            index: choice.index.unwrap_or(0),
            message: choice.message,
            finish_reason: choice.finish_reason.unwrap_or_else(|| "stop".to_string()),
            logprobs: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GitHubCopilotCompletionResponse {
    pub id: String,
    #[serde(default)]
    pub object: Option<String>,
    #[serde(default)]
    pub created: Option<u64>,
    pub model: String,
    pub choices: Vec<GitHubCopilotChoice>,
    #[serde(default)]
    pub usage: Option<rig::providers::openai::completion::Usage>,
    #[serde(default)]
    pub system_fingerprint: Option<String>,
}

impl From<GitHubCopilotCompletionResponse>
    for rig::providers::openai::completion::CompletionResponse
{
    fn from(response: GitHubCopilotCompletionResponse) -> Self {
        Self {
            id: response.id,
            object: response
                .object
                .unwrap_or_else(|| "chat.completion".to_string()),
            created: response.created.unwrap_or(0),
            model: response.model,
            choices: response.choices.into_iter().map(|c| c.into()).collect(),
            usage: response.usage,
            system_fingerprint: response.system_fingerprint,
        }
    }
}

#[cfg(test)]
#[path = "openai4x_test.rs"]
mod openai4x_test;
