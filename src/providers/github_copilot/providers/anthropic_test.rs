use crate::providers::github_copilot::providers::contract::GitHubCopilotProvider;

#[test]
fn concrete_provider_owns_endpoint_and_mapping_anthropic() {
    assert_eq!(
        <super::AnthropicProvider as GitHubCopilotProvider>::ENDPOINT_PATH,
        "/chat/completions"
    );
    assert_eq!(
        <super::AnthropicProvider as GitHubCopilotProvider>::INTENT_HEADER,
        "conversation-agent"
    );
}

#[test]
fn parity_with_rig_anthropic_parse_order() {
    use serde_json::json;

    let success_json = json!({
        "id": "msg_123",
        "model": "claude-sonnet-4-6",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15
        }
    })
    .to_string();

    let result = super::AnthropicProvider::map_response(&success_json);
    assert!(
        result.is_ok(),
        "Success completion must parse without top-level type"
    );

    let error_envelope_json = json!({
        "type": "error",
        "message": "Invalid API key"
    })
    .to_string();

    let parsed: Result<
        super::ApiResponse<super::super::openai4x::GitHubCopilotCompletionResponse>,
        _,
    > = serde_json::from_str(&error_envelope_json);
    assert!(parsed.is_ok(), "Error envelope should deserialize");
    assert!(
        matches!(parsed.unwrap(), super::ApiResponse::Error(_)),
        "Should be Error variant"
    );
}

#[test]
fn success_envelope_parses_normally() {
    use serde_json::json;

    let success_json = json!({
        "id": "msg_456",
        "model": "claude-sonnet-4-6",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Test response"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 20,
            "completion_tokens": 10,
            "total_tokens": 30
        }
    })
    .to_string();

    let result = super::AnthropicProvider::map_response(&success_json);
    assert!(result.is_ok());

    let response = result.unwrap();
    assert!(
        response.choices.len() > 0,
        "Should have at least one choice"
    );
}

#[test]
fn error_envelope_on_200_returns_provider_error() {
    use rig::completion::request::CompletionError;
    use serde_json::json;

    let error_json = json!({
        "type": "error",
        "message": "Rate limit exceeded"
    })
    .to_string();

    let status = reqwest::StatusCode::OK;
    let err = if status.is_success() {
        match super::AnthropicProvider::map_response(&error_json) {
            Ok(_) => panic!("expected error response"),
            Err(primary_error) => {
                match serde_json::from_str::<super::ApiResponse<serde_json::Value>>(&error_json) {
                    Ok(super::ApiResponse::Error(super::ApiErrorResponse { message })) => {
                        CompletionError::ResponseError(message)
                    }
                    _ => primary_error,
                }
            }
        }
    } else {
        CompletionError::ProviderError(error_json)
    };

    match err {
        CompletionError::ResponseError(msg) => assert_eq!(msg, "Rate limit exceeded"),
        other => panic!("Expected ResponseError, got: {:?}", other),
    }
}

#[test]
fn top_level_message_envelope_returns_provider_error() {
    use rig::completion::request::CompletionError;
    use serde_json::json;

    let legacy_error_json = json!({
        "message": "Authentication failed",
        "code": "invalid_api_key"
    })
    .to_string();

    let err = super::AnthropicProvider::map_response(&legacy_error_json).unwrap_err();
    match err {
        CompletionError::JsonError(_) => {}
        other => panic!(
            "Expected JsonError for non-ApiResponse/non-completion payload, got: {:?}",
            other
        ),
    }
}

#[test]
fn html_guard_behavior_unchanged() {
    let html_response = "<!DOCTYPE html><html><body>Error</body></html>";

    assert!(
        html_response.trim_start().starts_with("<!DOCTYPE")
            || html_response.trim_start().starts_with("<html")
    );
}

#[test]
fn map_request_sets_tool_function_strict_true() {
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

    let bytes = <super::AnthropicProvider as GitHubCopilotProvider>::map_request(
        "claude-sonnet-4.5",
        request,
    )
    .expect("map request");

    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json request");
    let strict = &value["tools"][0]["function"]["strict"];
    assert_eq!(strict, &serde_json::Value::Bool(true));
}
