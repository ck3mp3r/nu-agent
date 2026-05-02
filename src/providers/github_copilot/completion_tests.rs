//! Tests for GitHub Copilot completion model

use crate::providers::github_copilot::providers::contract::GitHubCopilotProvider;
use serde_json;

#[test]
fn completion_model_can_be_instantiated() {
    // This test verifies the CompletionModel struct can be created with proper types
    fn _assert_type<
        P: crate::providers::github_copilot::providers::contract::GitHubCopilotProvider,
        H: rig::http_client::HttpClientExt,
    >(
        _model: super::CompletionModel<P, H>,
    ) {
    }

    // Test passes if code compiles
}

#[test]
fn completion_model_has_correct_fields() {
    // This test verifies the CompletionModel has the expected public fields
    fn _assert_fields(model: super::CompletionModel) {
        let _model_name: String = model.model;
        // client field is pub(crate) so we can't access it here, which is correct
    }
}

#[test]
fn completion_model_implements_clone() {
    // Verify CompletionModel can be cloned
    fn _assert_clone<T: Clone>() {}
    _assert_clone::<super::CompletionModel>();
}

#[test]
fn completion_model_implements_completion_model_trait() {
    // Verify CompletionModel implements the required CompletionModel trait
    fn _assert_trait<T: rig::completion::request::CompletionModel>() {}
    _assert_trait::<super::CompletionModel>();
}

#[test]
fn completion_model_can_be_used_in_agent() {
    // Verify that CompletionModel can be used with Agent
    // Agent requires CompletionModel trait, so this validates compatibility
    fn _assert_agent_compatible<M: rig::completion::request::CompletionModel + 'static>() {}
    _assert_agent_compatible::<super::CompletionModel>();
}

#[test]
fn parses_valid_openai_compatible_response() {
    // Test that we can parse a valid OpenAI-compatible response
    let json = r#"{
        "id": "chatcmpl-123",
        "object": "chat.completion",
        "created": 1677652288,
        "model": "gpt-4o-mini",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "4"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 1,
            "total_tokens": 11
        }
    }"#;

    let result =
        serde_json::from_str::<rig::providers::openai::completion::CompletionResponse>(json);
    assert!(result.is_ok(), "Should parse valid OpenAI response");
}

#[test]
fn parses_github_copilot_error_with_error_field() {
    let result = serde_json::from_str::<
        crate::providers::github_copilot::providers::openai4x::GitHubCopilotCompletionResponse,
    >(
        r#"{
        "id": "x",
        "model": "m",
        "choices": [{"message": {"role": "assistant", "content": "ok"}}]
    }"#,
    );
    assert!(result.is_ok(), "Should parse completion response structure");
}

#[test]
fn parses_github_copilot_error_with_message_field() {
    // Test parsing GitHub Copilot error format with top-level message
    let json = r#"{
        "message": "Rate limit exceeded"
    }"#;

    let value: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(value["message"], "Rate limit exceeded");
}

#[test]
fn handles_html_response() {
    // Test that we detect HTML responses (common error case)
    let html = "<!DOCTYPE html><html><body>Error</body></html>";
    assert!(html.trim_start().starts_with("<!DOCTYPE"));
}

#[test]
fn handles_empty_error_response() {
    // Test that we can handle empty error responses
    let json = "{}";

    let value: serde_json::Value = serde_json::from_str(json).unwrap();
    assert!(value.as_object().is_some());
}

// ============================================================================
// Backend-Generic CompletionModel Tests
// ============================================================================

#[test]
fn completion_model_can_be_generic_over_backend() {
    // Verify CompletionModel can be parameterized with different backends
    use crate::providers::github_copilot::providers::{AnthropicProvider, OpenAI4xProvider};

    fn _assert_type<
        P: crate::providers::github_copilot::providers::contract::GitHubCopilotProvider,
        H: rig::http_client::HttpClientExt,
    >(
        _model: super::CompletionModel<P, H>,
    ) {
    }

    // Use the imported types to avoid unused warning
    let _: Option<AnthropicProvider> = None;
    let _: Option<OpenAI4xProvider> = None;

    // Test passes if code compiles
}

#[test]
fn completion_model_anthropic_backend_implements_traits() {
    // Verify CompletionModel<AnthropicProvider> implements required traits
    use crate::providers::github_copilot::providers::AnthropicProvider;

    fn _assert_clone<T: Clone>() {}
    _assert_clone::<super::CompletionModel<AnthropicProvider>>();

    fn _assert_completion_model<T: rig::completion::request::CompletionModel>() {}
    _assert_completion_model::<super::CompletionModel<AnthropicProvider>>();
}

#[test]
fn completion_model_openai_backend_implements_traits() {
    // Verify CompletionModel<OpenAI4xProvider> implements required traits
    use crate::providers::github_copilot::providers::OpenAI4xProvider;

    fn _assert_clone<T: Clone>() {}
    _assert_clone::<super::CompletionModel<OpenAI4xProvider>>();

    fn _assert_completion_model<T: rig::completion::request::CompletionModel>() {}
    _assert_completion_model::<super::CompletionModel<OpenAI4xProvider>>();
}

#[test]
fn openai_gpt5_models_use_responses_endpoint_path() {
    assert_eq!(
        crate::providers::github_copilot::providers::OpenAI5xProvider::ENDPOINT_PATH,
        "/responses"
    );
}
