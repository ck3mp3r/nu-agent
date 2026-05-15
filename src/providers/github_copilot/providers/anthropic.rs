use crate::providers::github_copilot::providers::contract::{
    CopilotCompletion, CopilotResponse, GitHubCopilotProvider,
};
use rig::completion::request::{CompletionError, CompletionRequest as CoreCompletionRequest};
use rig::http_client::{self, HeaderValue, HttpClientExt};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Default, Clone, Copy)]
pub struct AnthropicProvider;

impl GitHubCopilotProvider for AnthropicProvider {
    const NAME: &'static str = "AnthropicProvider";
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
        let response =
            serde_json::from_str::<super::openai4x::GitHubCopilotCompletionResponse>(text)?;
        Ok(response.into())
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

            // HTML guard (transport-only delta, allowed by task requirements)
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
                match Self::map_response(&text) {
                    Ok(parsed) => parsed.try_into(),
                    Err(primary_error) => match serde_json::from_str::<ApiResponse<Value>>(&text) {
                        Ok(ApiResponse::Error(ApiErrorResponse { message })) => {
                            Err(CompletionError::ResponseError(message))
                        }
                        _ => Err(primary_error),
                    },
                }
            } else {
                Err(CompletionError::ProviderError(text))
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiErrorResponse {
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ApiResponse<T> {
    Message(T),
    Error(ApiErrorResponse),
}

#[cfg(test)]
#[path = "anthropic_test.rs"]
mod anthropic_test;
