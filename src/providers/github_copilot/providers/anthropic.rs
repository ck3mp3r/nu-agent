use crate::providers::github_copilot::providers::contract::{
    CopilotCompletion, CopilotResponse, GitHubCopilotProvider,
};
use rig::completion::request::{CompletionError, CompletionRequest as CoreCompletionRequest};
use rig::http_client::{self, HeaderValue, HttpClientExt};
use serde::Deserialize;

#[derive(Debug, Default, Clone, Copy)]
pub struct AnthropicProvider;

impl GitHubCopilotProvider for AnthropicProvider {
    const NAME: &'static str = "AnthropicProvider";
    const ENDPOINT_PATH: &'static str = "/chat/completions";
    const INTENT_HEADER: &'static str = "conversation-panel";

    fn map_request(
        model: &str,
        completion_request: CoreCompletionRequest,
    ) -> Result<Vec<u8>, CompletionError> {
        let request = rig::providers::openai::completion::CompletionRequest::try_from(
            rig::providers::openai::completion::OpenAIRequestParams {
                model: model.to_owned(),
                request: completion_request,
                strict_tools: false,
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

    fn map_error(status: reqwest::StatusCode, text: &str) -> CompletionError {
        match serde_json::from_str::<GitHubCopilotError>(text) {
            Ok(err_response) => {
                let error_msg = err_response
                    .error
                    .map(|e| e.message)
                    .or(err_response.message)
                    .unwrap_or_else(|| text.to_string());
                CompletionError::ProviderError(format!(
                    "{} {} HTTP {}: {}",
                    Self::NAME,
                    Self::ENDPOINT_PATH,
                    status,
                    error_msg
                ))
            }
            Err(_) => CompletionError::ProviderError(format!(
                "{} {} HTTP {}: {}",
                Self::NAME,
                Self::ENDPOINT_PATH,
                status,
                text
            )),
        }
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
}

#[cfg(test)]
#[path = "anthropic_tests.rs"]
mod anthropic_tests;
