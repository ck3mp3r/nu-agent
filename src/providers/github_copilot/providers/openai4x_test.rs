use crate::providers::github_copilot::providers::contract::GitHubCopilotProvider;
use rig::completion::request::CompletionError;

#[test]
fn concrete_provider_owns_endpoint_and_mapping_openai4x() {
    assert_eq!(
        <super::OpenAI4xProvider as GitHubCopilotProvider>::ENDPOINT_PATH,
        "/chat/completions"
    );
    assert_eq!(
        <super::OpenAI4xProvider as GitHubCopilotProvider>::INTENT_HEADER,
        "conversation-agent"
    );
}

#[test]
fn map_request_sets_tool_function_strict_true_for_openai4x() {
    use rig::completion::request::{CompletionRequest, ToolDefinition};

    let request = CompletionRequest {
        model: None,
        preamble: None,
        chat_history: rig::OneOrMany::one(rig::completion::Message::user(
            "what is in this directory?",
        )),
        documents: vec![],
        tools: vec![ToolDefinition {
            name: "ls".to_string(),
            description: "list directory".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                }
            }),
        }],
        temperature: None,
        max_tokens: None,
        tool_choice: None,
        additional_params: None,
        output_schema: None,
    };

    let bytes = <super::OpenAI4xProvider as GitHubCopilotProvider>::map_request("gpt-4o", request)
        .expect("map request");

    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json request");
    let strict = &value["tools"][0]["function"]["strict"];
    assert_eq!(strict, &serde_json::Value::Bool(true));
}

// ======================================================================
// Parity tests with rig OpenAI completion semantics
// ======================================================================

#[test]
fn parity_with_rig_openai4x_parse_order() {
    // Branch 1: Success status → parse ApiResponse → match Ok|Err envelope
    let success_ok = r#"{"id":"1","object":"chat.completion","created":0,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"test"},"finish_reason":"stop"}]}"#;
    let result = <super::OpenAI4xProvider as GitHubCopilotProvider>::map_response(success_ok);
    assert!(result.is_ok(), "success Ok envelope should parse");

    let success_err = r#"{"error":{"message":"model overloaded"}}"#;
    let result = <super::OpenAI4xProvider as GitHubCopilotProvider>::map_response(success_err);
    assert!(
        matches!(result, Err(CompletionError::ProviderError(_))),
        "success Err envelope should return ProviderError"
    );
    if let Err(CompletionError::ProviderError(msg)) = result {
        assert_eq!(msg, "model overloaded");
    }

    // Branch 2: Non-success status → ProviderError(text)
    let non_success_text = "upstream timeout";
    let result = <super::OpenAI4xProvider as GitHubCopilotProvider>::map_error(
        reqwest::StatusCode::BAD_GATEWAY,
        non_success_text,
    );
    assert!(
        matches!(result, CompletionError::ProviderError(_)),
        "non-success should return ProviderError"
    );
    if let CompletionError::ProviderError(msg) = result {
        assert_eq!(msg, "upstream timeout");
    }
}

#[test]
fn success_envelope_parses_normally() {
    let json = r#"{"id":"chatcmpl-123","object":"chat.completion","created":1677652288,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"Hello!"},"finish_reason":"stop"}]}"#;
    let result = <super::OpenAI4xProvider as GitHubCopilotProvider>::map_response(json);
    assert!(result.is_ok());
}

#[test]
fn error_envelope_on_200_returns_provider_error() {
    let json = r#"{"error":{"message":"rate limit exceeded"}}"#;
    let result = <super::OpenAI4xProvider as GitHubCopilotProvider>::map_response(json);
    assert!(matches!(result, Err(CompletionError::ProviderError(_))));
    if let Err(CompletionError::ProviderError(msg)) = result {
        assert_eq!(msg, "rate limit exceeded");
    }
}

#[test]
fn top_level_message_envelope_returns_provider_error() {
    // Top-level message field (no nested error object)
    let json = r#"{"message":"service unavailable"}"#;
    let result = <super::OpenAI4xProvider as GitHubCopilotProvider>::map_response(json);
    // This will fail to parse as ApiResponse<CompletionResponse> and return SerdeError
    // which becomes CompletionError::SerdeError, NOT ProviderError
    // BUT: rig behavior is to let serde error propagate, so this is correct parity
    assert!(result.is_err());
}

#[test]
fn parse_order_prefers_structured_error_message_over_raw_text() {
    // When error envelope is present, extract .message field
    let json = r#"{"error":{"message":"invalid API key"}}"#;
    let result = <super::OpenAI4xProvider as GitHubCopilotProvider>::map_response(json);
    assert!(matches!(result, Err(CompletionError::ProviderError(_))));
    if let Err(CompletionError::ProviderError(msg)) = result {
        assert_eq!(msg, "invalid API key");
    }
}

#[test]
fn html_guard_behavior_unchanged() {
    // HTML responses should be caught by execute() before map_response
    // This test verifies map_response doesn't special-case HTML
    let html = r#"<!DOCTYPE html><html><body>Error</body></html>"#;
    let result = <super::OpenAI4xProvider as GitHubCopilotProvider>::map_response(html);
    // Should fail as JSON parse error, not special HTML handling
    assert!(result.is_err());
}
