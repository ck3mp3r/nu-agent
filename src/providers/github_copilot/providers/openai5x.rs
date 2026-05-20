use crate::providers::github_copilot::providers::contract::{
    CopilotCompletion, CopilotResponse, GitHubCopilotProvider,
};
use rig::completion::Usage;
use rig::completion::message::{AssistantContent, Text};
use rig::completion::request::{CompletionError, CompletionRequest as CoreCompletionRequest};
use rig::http_client::{self, HeaderValue, HttpClientExt};
use rig::one_or_many::OneOrMany;
use serde::Deserialize;

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
        serde_json::to_vec(&request).map_err(Into::into)
    }

    fn map_response(text: &str) -> Result<CopilotResponse, CompletionError> {
        let response = Self::parse_and_validate(text)?;
        Self::build_raw_response(&response)
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
                Self::build_completion(&text)
            } else {
                Err(Self::map_error(status, &text))
            }
        }
    }
}

/// Private struct to hold validated tool call data
struct ValidatedToolCall {
    id: String,      // fc_-prefixed
    call_id: String, // call_-prefixed
    name: String,
    arguments: serde_json::Value,
}

impl OpenAI5xProvider {
    /// Parse and validate a ResponsesApiResponse from JSON text.
    ///
    /// This performs three validation steps mirroring rig's responses_api semantics:
    /// 1. Status gate: Only `Completed` status proceeds, others error with descriptive messages
    /// 2. Error field check: Even on `Completed` status, error field causes failure
    /// 3. Empty output check: Response must contain at least one output item
    ///
    /// Source: rig-core/src/providers/openai/responses_api/websocket.rs:617-640
    ///         rig-core/src/providers/openai/responses_api/mod.rs:1448-1451
    fn parse_and_validate(text: &str) -> Result<ResponsesApiResponse, CompletionError> {
        let response = serde_json::from_str::<ResponsesApiResponse>(text)?;

        // Status gate: mirror rig responses_api status gating semantics exactly
        match response.status {
            ResponseStatus::Completed => {
                // Proceed to conversion
            }
            ResponseStatus::Failed => {
                let error_msg = if let Some(ref error) = response.error {
                    if error.code.is_empty() {
                        error.message.clone()
                    } else {
                        format!("{}: {}", error.code, error.message)
                    }
                } else {
                    "OpenAI responses returned a failed response".to_string()
                };
                return Err(CompletionError::ProviderError(error_msg));
            }
            ResponseStatus::Incomplete => {
                let reason = response
                    .incomplete_details
                    .as_ref()
                    .map(|details| details.reason.as_str())
                    .unwrap_or("unknown reason");
                return Err(CompletionError::ProviderError(format!(
                    "OpenAI responses response was incomplete: {reason}"
                )));
            }
            status => {
                return Err(CompletionError::ProviderError(format!(
                    "OpenAI responses response ended with status {:?}",
                    status
                )));
            }
        }

        // Check for error field even on completed status (rig semantics)
        if let Some(ref error) = response.error {
            let error_msg = if error.code.is_empty() {
                error.message.clone()
            } else {
                format!("{}: {}", error.code, error.message)
            };
            return Err(CompletionError::ProviderError(error_msg));
        }

        // Empty output check: mirror rig TryFrom semantics
        if response.output.is_empty() {
            return Err(CompletionError::ResponseError(
                "Response contained no parts".to_owned(),
            ));
        }

        Ok(response)
    }

    /// Extract message text from a ResponsesApiResponse.
    /// Combines all output_text content from message items.
    fn extract_text(response: &ResponsesApiResponse) -> String {
        let message_text_parts: Vec<String> = response
            .output
            .iter()
            .filter(|item| item.kind == "message")
            .flat_map(|item| item.content.iter().flatten())
            .filter(|content| content.kind == "output_text")
            .filter_map(|content| content.text.clone())
            .collect();
        message_text_parts.join("\n")
    }

    /// Extract and validate tool calls from a ResponsesApiResponse.
    ///
    /// Mirrors rig OutputFunctionCall semantics exactly:
    /// - call_id is REQUIRED (String, not Option)
    /// - arguments uses stringified_json deserializer (empty string → {})
    /// - No tool call drop on validation - all errors are deserialization failures
    ///
    /// Source: rig-core/src/providers/openai/responses_api/mod.rs:1247-1255, 1296-1303
    fn extract_tool_calls(
        response: &ResponsesApiResponse,
    ) -> Result<Vec<ValidatedToolCall>, CompletionError> {
        response
            .output
            .iter()
            .filter(|item| item.kind == "function_call")
            .map(|item| {
                // Validate call_id requirement (rig line 1300: pub call_id: String)
                let call_id = item.call_id.clone().ok_or_else(|| {
                    CompletionError::ProviderError(
                        "Function call missing required call_id field".to_string(),
                    )
                })?;

                // Validate id requirement (rig uses both id and call_id)
                let id = item.id.clone().ok_or_else(|| {
                    CompletionError::ProviderError(
                        "Function call missing required id field".to_string(),
                    )
                })?;

                // Validate name requirement
                let name = item.name.clone().ok_or_else(|| {
                    CompletionError::ProviderError(
                        "Function call missing required name field".to_string(),
                    )
                })?;

                // Handle arguments (rig json_utils::stringified_json semantics)
                // Source: rig-core/src/json_utils.rs:63-72
                // - empty/whitespace string → {}
                // - valid JSON string → parse
                // - malformed JSON → error
                let arguments_str = item.arguments.clone().unwrap_or_default();
                let arguments = if arguments_str.trim().is_empty() {
                    serde_json::json!({})
                } else {
                    serde_json::from_str(&arguments_str).map_err(|e| {
                        CompletionError::ProviderError(format!(
                            "Malformed function call arguments JSON: {}",
                            e
                        ))
                    })?
                };

                Ok(ValidatedToolCall {
                    id,
                    call_id,
                    name,
                    arguments,
                })
            })
            .collect()
    }

    /// Build CopilotResponse (raw_response) from a parsed ResponsesApiResponse.
    /// Constructs Chat Completions JSON format without re-parsing the input text.
    fn build_raw_response(
        response: &ResponsesApiResponse,
    ) -> Result<CopilotResponse, CompletionError> {
        let combined_text = Self::extract_text(response);
        let tool_calls = Self::extract_tool_calls(response)?;

        // Convert ValidatedToolCall to Chat Completions JSON format
        let tool_calls_json: Vec<serde_json::Value> = tool_calls
            .iter()
            .map(|tc| {
                serde_json::json!({
                    "id": tc.id,
                    "call_id": tc.call_id,
                    "type": "function",
                    "function": {
                        "name": tc.name,
                        "arguments": tc.arguments.to_string()
                    }
                })
            })
            .collect();

        // Empty content check: mirror rig TryFrom semantics
        if combined_text.trim().is_empty() && tool_calls_json.is_empty() {
            return Err(CompletionError::ResponseError(
                "Response contained no message or tool call (empty)".to_owned(),
            ));
        }

        // Build assistant message JSON
        let assistant_message_json = if combined_text.trim().is_empty() {
            serde_json::json!({
                "role": "assistant",
                "content": "",
                "tool_calls": tool_calls_json
            })
        } else if tool_calls_json.is_empty() {
            serde_json::json!({
                "role": "assistant",
                "content": combined_text
            })
        } else {
            serde_json::json!({
                "role": "assistant",
                "content": combined_text,
                "tool_calls": tool_calls_json
            })
        };

        // Build usage
        let default_usage = ResponsesUsage::default();
        let usage_data = response.usage.as_ref().unwrap_or(&default_usage);
        let usage_json = serde_json::json!({
            "prompt_tokens": usage_data.input_tokens.unwrap_or(0),
            "completion_tokens": usage_data.output_tokens.unwrap_or(0),
            "total_tokens": usage_data.total_tokens.unwrap_or(0)
        });

        // Build the full response JSON
        let response_json = serde_json::json!({
            "id": response.id,
            "object": "chat.completion",
            "created": 0,
            "model": response.model,
            "choices": [{
                "index": 0,
                "message": assistant_message_json,
                "finish_reason": "stop"
            }],
            "usage": usage_json
        });

        // Deserialize into rig's CompletionResponse
        serde_json::from_value(response_json).map_err(Into::into)
    }

    pub fn build_completion(text: &str) -> Result<CopilotCompletion, CompletionError> {
        let response = Self::parse_and_validate(text)?;

        // Extract text and tool calls using shared helpers
        let combined_text = Self::extract_text(&response);
        let tool_calls = Self::extract_tool_calls(&response)?;

        // Build AssistantContent from validated tool calls
        let tool_call_contents: Vec<AssistantContent> = tool_calls
            .into_iter()
            .map(|tc| {
                AssistantContent::tool_call_with_call_id(tc.id, tc.call_id, tc.name, tc.arguments)
            })
            .collect();

        // Build content parts
        let mut content_parts: Vec<AssistantContent> = Vec::new();

        if !combined_text.trim().is_empty() {
            content_parts.push(AssistantContent::Text(Text {
                text: combined_text,
            }));
        }

        content_parts.extend(tool_call_contents);

        // Empty content check: mirror rig TryFrom semantics
        if content_parts.is_empty() {
            return Err(CompletionError::ResponseError(
                "Response contained no message or tool call (empty)".to_owned(),
            ));
        }

        // Build OneOrMany<AssistantContent>
        let choice = if content_parts.len() == 1 {
            OneOrMany::one(content_parts.into_iter().next().unwrap())
        } else {
            OneOrMany::many(content_parts).map_err(|_| {
                CompletionError::ResponseError(
                    "Failed to create OneOrMany from content parts".to_owned(),
                )
            })?
        };

        // Build Usage
        let default_usage = ResponsesUsage::default();
        let usage_data = response.usage.as_ref().unwrap_or(&default_usage);
        let usage = Usage {
            input_tokens: usage_data.input_tokens.unwrap_or(0),
            output_tokens: usage_data.output_tokens.unwrap_or(0),
            total_tokens: usage_data.total_tokens.unwrap_or(0),
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
            reasoning_tokens: 0,
        };

        // Get raw_response without double-parsing - reuse already-parsed response
        let raw_response = Self::build_raw_response(&response)?;

        Ok(CopilotCompletion {
            choice,
            usage,
            raw_response,
            message_id: None,
        })
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

#[derive(Debug, Deserialize)]
struct ResponsesApiResponse {
    id: String,
    model: String,
    #[serde(default)]
    status: ResponseStatus,
    #[serde(default)]
    error: Option<ResponseError>,
    #[serde(default)]
    incomplete_details: Option<IncompleteDetailsReason>,
    #[serde(default)]
    output: Vec<ResponsesOutputItem>,
    #[serde(default)]
    usage: Option<ResponsesUsage>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum ResponseStatus {
    #[default]
    Completed,
    Failed,
    Incomplete,
    InProgress,
    Cancelled,
    Queued,
}

#[derive(Debug, Deserialize)]
struct ResponseError {
    #[serde(default)]
    code: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct IncompleteDetailsReason {
    reason: String,
}

#[derive(Debug, Deserialize)]
struct ResponsesOutputItem {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    id: Option<String>,
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
#[path = "openai5x_test.rs"]
mod openai5x_test;
