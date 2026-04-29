use crate::providers::github_copilot::providers::contract::{
    CopilotCompletion, CopilotResponse, GitHubCopilotProvider,
};
use rig::completion::request::{CompletionError, CompletionRequest as CoreCompletionRequest};
use rig::http_client::{self, HeaderValue, HttpClientExt};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAI5xProvider;

impl GitHubCopilotProvider for OpenAI5xProvider {
    const NAME: &'static str = "OpenAI5xProvider";
    const ENDPOINT_PATH: &'static str = "/responses";
    const INTENT_HEADER: &'static str = "conversation-agent";

    fn map_request(
        model: &str,
        completion_request: CoreCompletionRequest,
    ) -> Result<Vec<u8>, CompletionError> {
        let request = rig::providers::openai::responses_api::CompletionRequest::try_from((
            model.to_owned(),
            completion_request,
        ))?;
        let mut value = serde_json::to_value(request)?;

        if let Some(input_value) = value.get_mut("input")
            && !input_value.is_string()
        {
            *input_value = Value::String(coerce_input_to_string(input_value));
        }

        serde_json::to_vec(&value).map_err(Into::into)
    }

    fn map_response(text: &str) -> Result<CopilotResponse, CompletionError> {
        let response = serde_json::from_str::<ResponsesApiResponse>(text)?;

        let message_text = response
            .output
            .iter()
            .filter(|item| item.kind == "message")
            .flat_map(|item| item.content.iter().flatten())
            .filter(|content| content.kind == "output_text")
            .filter_map(|content| content.text.clone())
            .collect::<Vec<_>>()
            .join("\n");

        let tool_calls = response
            .output
            .iter()
            .filter(|item| item.kind == "function_call")
            .filter_map(|item| {
                let id = item.call_id.clone()?;
                let name = item.name.clone()?;
                let args = item.arguments.clone().unwrap_or_else(|| "{}".to_string());
                Some(serde_json::json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": args
                    }
                }))
            })
            .collect::<Vec<_>>();

        let assistant_message = if message_text.trim().is_empty() {
            serde_json::json!({
                "role": "assistant",
                "content": "",
                "tool_calls": tool_calls
            })
        } else if tool_calls.is_empty() {
            serde_json::json!({
                "role": "assistant",
                "content": message_text
            })
        } else {
            serde_json::json!({
                "role": "assistant",
                "content": message_text,
                "tool_calls": tool_calls
            })
        };

        let usage = response.usage.unwrap_or_default();
        let value = serde_json::json!({
            "id": response.id,
            "object": "chat.completion",
            "created": 0,
            "model": response.model,
            "choices": [
                {
                    "index": 0,
                    "message": assistant_message,
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": usage.input_tokens.unwrap_or(0),
                "completion_tokens": usage.output_tokens.unwrap_or(0),
                "total_tokens": usage.total_tokens.unwrap_or(0)
            }
        });

        serde_json::from_value(value).map_err(Into::into)
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

            if status.is_success() {
                let parsed = Self::map_response(&text)?;
                parsed.try_into()
            } else {
                Err(Self::map_error(status, &text))
            }
        }
    }
}

fn coerce_input_to_string(input: &Value) -> String {
    if let Some(s) = input.as_str() {
        return s.to_string();
    }

    if let Some(arr) = input.as_array() {
        return arr
            .iter()
            .filter_map(extract_text_fragment)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
    }

    extract_text_fragment(input).unwrap_or_default()
}

fn extract_text_fragment(value: &Value) -> Option<String> {
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return Some(text.to_string());
    }

    if let Some(content) = value.get("content") {
        if let Some(text) = content.as_str() {
            return Some(text.to_string());
        }
        if let Some(arr) = content.as_array() {
            let merged = arr
                .iter()
                .filter_map(extract_text_fragment)
                .collect::<Vec<_>>()
                .join("\n");
            if !merged.is_empty() {
                return Some(merged);
            }
        }
    }

    None
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

#[derive(Debug, Deserialize)]
struct ResponsesApiResponse {
    id: String,
    model: String,
    #[serde(default)]
    output: Vec<ResponsesOutputItem>,
    #[serde(default)]
    usage: Option<ResponsesUsage>,
}

#[derive(Debug, Deserialize)]
struct ResponsesOutputItem {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
    #[serde(default)]
    content: Option<Vec<ResponsesOutputContent>>,
}

#[derive(Debug, Deserialize)]
struct ResponsesOutputContent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ResponsesUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    total_tokens: Option<u64>,
}

#[cfg(test)]
#[path = "test.rs"]
mod test;
