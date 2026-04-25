use crate::config::Config;
use crate::llm::{call_llm, format_response};
use nu_protocol::Span;
use std::sync::Mutex;
use wiremock::{
    matchers::{header, method},
    Mock, MockServer, ResponseTemplate,
};

// Global mutex to ensure rig client isolation between tests
// This works around a rig bug where OpenAI/Anthropic/Ollama clients share state
static RIG_TEST_LOCK: Mutex<()> = Mutex::new(());

// Note: These tests use serial_test to avoid environment variable conflicts

/// Helper to create a mock OpenAI-compatible response
/// Helper to create a mock OpenAI response
/// Using exact OpenAI Chat Completions API format
fn openai_mock_response() -> String {
    // Return raw JSON string to ensure exact format
    r#"{
  "id": "chatcmpl-123",
  "object": "chat.completion",
  "created": 1677652288,
  "model": "gpt-4-0613",
  "choices": [{
    "index": 0,
    "message": {
      "role": "assistant",
      "content": "This is a mocked response from the LLM."
    },
    "logprobs": null,
    "finish_reason": "stop"
  }],
  "usage": {
    "prompt_tokens": 9,
    "completion_tokens": 12,
    "total_tokens": 21
  }
}"#.to_string()
}

/// Helper to create a mock Ollama response
/// Ollama uses a slightly different format with created_at as string (not integer)
fn ollama_mock_response() -> String {
    r#"{
  "id": "chatcmpl-123",
  "object": "chat.completion",
  "created_at": "2023-03-01T00:00:00Z",
  "model": "llama2",
  "choices": [{
    "index": 0,
    "message": {
      "role": "assistant",
      "content": "This is a mocked response from the LLM."
    },
    "logprobs": null,
    "finish_reason": "stop"
  }],
  "usage": {
    "prompt_tokens": 9,
    "completion_tokens": 12,
    "total_tokens": 21
  }
}"#.to_string()
}

/// Helper to create a mock Anthropic response
/// Based on Anthropic Messages API format
fn anthropic_mock_response() -> String {
    r#"{
  "id": "msg_01XFDUDYJgAACzvnptvVoYEL",
  "type": "message",
  "role": "assistant",
  "content": [{
    "type": "text",
    "text": "This is a mocked response from the LLM."
  }],
  "model": "claude-3-opus-20240229",
  "stop_reason": "end_turn",
  "stop_sequence": null,
  "usage": {
    "input_tokens": 10,
    "output_tokens": 20
  }
}"#.to_string()
}

#[tokio::test]
#[serial_test::serial]
async fn test_call_llm_anthropic_from_env() {
    // Acquire lock to ensure rig client isolation
    let _guard = RIG_TEST_LOCK.lock().unwrap();
    
    // Set up mock server
    let mock_server = MockServer::start().await;
    
    // Configure mock response for Anthropic
    Mock::given(method("POST"))
        .and(header("x-api-key", "sk-ant-test-key"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_string(anthropic_mock_response())
            .insert_header("content-type", "application/json"))
        .mount(&mock_server)
        .await;

    unsafe {
        std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test-key");
    }

    let config = Config {
        provider: "anthropic".to_string(),
        provider_impl: None,
        model: "claude-3-opus".to_string(),
        api_key: None,
        base_url: Some(mock_server.uri()),
        temperature: None,
        max_tokens: Some(1024),  // Anthropic requires max_tokens
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    };

    let result = call_llm(&config, "test prompt").await;

    unsafe {
        std::env::remove_var("ANTHROPIC_API_KEY");
    }

    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());
    let response = result.unwrap();
    assert_eq!(response, "This is a mocked response from the LLM.");
    
    // Explicitly drop mock server and add delay to ensure cleanup
    drop(mock_server);
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
}

// NOTE: This test must run individually due to a rig bug where OpenAI client shares state
// when base_url is overridden. Run with: cargo test --lib test_call_llm_openai_from_env -- --exact --ignored
#[tokio::test]
#[serial_test::serial]
#[ignore = "rig bug: must run individually"]
async fn test_call_llm_openai_from_env() {
    // Acquire lock to ensure rig client isolation
    let _guard = RIG_TEST_LOCK.lock().unwrap();
    
    // Set up mock server
    let mock_server = MockServer::start().await;
    
    // Configure mock response
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_string(openai_mock_response())
            .insert_header("content-type", "application/json"))
        .mount(&mock_server)
        .await;

    unsafe {
        std::env::set_var("OPENAI_API_KEY", "sk-test-key");
    }

    let config = Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None, // Should use env var
        base_url: Some(mock_server.uri()),
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    };

    let result = call_llm(&config, "test prompt").await;

    unsafe {
        std::env::remove_var("OPENAI_API_KEY");
    }

    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());
    let response = result.unwrap();
    assert_eq!(response, "This is a mocked response from the LLM.");
    
    // Explicitly drop mock server and add delay to ensure cleanup
    drop(mock_server);
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
}

// NOTE: This test must run individually due to a rig bug where OpenAI client shares state
// when base_url is overridden. Run with: cargo test --lib test_call_llm_openai_with_api_key_override -- --exact --ignored
#[tokio::test]
#[serial_test::serial]
#[ignore = "rig bug: must run individually"]
async fn test_call_llm_openai_with_api_key_override() {
    // Acquire lock to ensure rig client isolation
    let _guard = RIG_TEST_LOCK.lock().unwrap();
    
    // Set up mock server
    let mock_server = MockServer::start().await;
    
    // Configure mock response with explicit API key
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer sk-override-key"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_string(openai_mock_response())
            .insert_header("content-type", "application/json"))
        .mount(&mock_server)
        .await;

    let config = Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: Some("sk-override-key".to_string()),
        base_url: Some(mock_server.uri()),
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    };

    let result = call_llm(&config, "test prompt").await;

    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());
    let response = result.unwrap();
    assert_eq!(response, "This is a mocked response from the LLM.");
    
    // Explicitly drop mock server and add delay to ensure cleanup
    drop(mock_server);
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
}

// NOTE: This test must run individually due to a rig bug where OpenAI client shares state
// when base_url is overridden. Run with: cargo test --lib test_call_llm_openai_with_base_url_override -- --exact --ignored
#[tokio::test]
#[serial_test::serial]
#[ignore = "rig bug: must run individually"]
async fn test_call_llm_openai_with_base_url_override() {
    // Acquire lock to ensure rig client isolation
    let _guard = RIG_TEST_LOCK.lock().unwrap();
    
    // Set up mock server for custom OpenAI-compatible API
    let mock_server = MockServer::start().await;
    
    // Configure mock response for OpenAI-compatible endpoint
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer sk-custom-key"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_string(openai_mock_response())
            .insert_header("content-type", "application/json"))
        .mount(&mock_server)
        .await;

    let config = Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "custom-model".to_string(),
        api_key: Some("sk-custom-key".to_string()),
        base_url: Some(mock_server.uri()),
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    };

    let result = call_llm(&config, "test prompt").await;

    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());
    let response = result.unwrap();
    assert_eq!(response, "This is a mocked response from the LLM.");
    
    // Explicitly drop mock server and add delay to ensure cleanup
    drop(mock_server);
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
}

#[tokio::test]
#[serial_test::serial]
async fn test_call_llm_missing_api_key() {
    unsafe {
        std::env::remove_var("OPENAI_API_KEY");
    }

    let config = Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    };

    let result = call_llm(&config, "test prompt").await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(err.msg.contains("Missing API key") || err.msg.contains("OPENAI_API_KEY"));
}

#[tokio::test]
async fn test_call_llm_unsupported_provider() {
    // RED: Should return error for unsupported provider
    let config = Config {
        provider: "unsupported".to_string(),
        provider_impl: None,
        model: "some-model".to_string(),
        api_key: Some("key".to_string()),
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    };

    let result = call_llm(&config, "test prompt").await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(err.msg.contains("Unsupported provider"));
}

// NOTE: This test must run individually due to a rig bug where Ollama client shares state
// when base_url is overridden. Run with: cargo test --lib test_call_llm_ollama -- --exact --ignored
#[tokio::test]
#[serial_test::serial]
#[ignore = "rig bug: must run individually"]
async fn test_call_llm_ollama() {
    // Acquire lock to ensure rig client isolation
    let _guard = RIG_TEST_LOCK.lock().unwrap();
    
    // Set up mock server for Ollama
    let mock_server = MockServer::start().await;
    
    // Ollama uses OpenAI-compatible API format with slight differences
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_string(ollama_mock_response())
            .insert_header("content-type", "application/json"))
        .mount(&mock_server)
        .await;

    let config = Config {
        provider: "ollama".to_string(),
        provider_impl: None,
        model: "llama2".to_string(),
        api_key: None,
        base_url: Some(mock_server.uri()),
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    };

    let result = call_llm(&config, "test prompt").await;

    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());
    let response = result.unwrap();
    assert_eq!(response, "This is a mocked response from the LLM.");
    
    // Explicitly drop mock server and add delay to ensure cleanup
    drop(mock_server);
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
}

// Tests for format_response

#[test]
fn test_format_response_basic() {
    let config = Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    };

    let response = "This is a test response from the LLM.";
    let value = format_response(response, &config, Span::unknown());

    // Verify it's a record
    let record = value.as_record().expect("Should be a record");

    // Check fields
    assert!(record.contains("response"));
    assert!(record.contains("model"));
    assert!(record.contains("provider"));
    assert!(record.contains("timestamp"));

    // Check values
    assert_eq!(record.get("response").unwrap().as_str().unwrap(), response);
    assert_eq!(record.get("model").unwrap().as_str().unwrap(), "gpt-4");
    assert_eq!(record.get("provider").unwrap().as_str().unwrap(), "openai");

    // Check timestamp is ISO8601 format
    let timestamp = record.get("timestamp").unwrap().as_str().unwrap();
    assert!(timestamp.contains("T"));
    assert!(timestamp.contains("Z") || timestamp.contains("+") || timestamp.contains("-"));
}

#[test]
fn test_format_response_empty() {
    let config = Config {
        provider: "anthropic".to_string(),
        provider_impl: None,
        model: "claude-3-opus".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    };

    let response = "";
    let value = format_response(response, &config, Span::unknown());

    let record = value.as_record().expect("Should be a record");
    assert_eq!(record.get("response").unwrap().as_str().unwrap(), "");
}

// ============================================================================
// GitHub Copilot Backend Routing Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_call_llm_github_copilot_anthropic_backend() {
    // Set up mock server for GitHub Copilot
    let mock_server = MockServer::start().await;
    
    // GitHub Copilot uses OpenAI-compatible API format
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer ghp-test-token"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_string(openai_mock_response())
            .insert_header("content-type", "application/json"))
        .mount(&mock_server)
        .await;

    unsafe {
        std::env::set_var("GITHUB_TOKEN", "ghp-test-token");
    }

    let config = Config {
        provider: "github-copilot/anthropic".to_string(),
        provider_impl: Some("openai".to_string()), // GitHub Copilot uses OpenAI API
        model: "claude-sonnet-4.5".to_string(),
        api_key: None,
        base_url: Some(mock_server.uri()),
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    };

    let result = call_llm(&config, "test prompt").await;

    unsafe {
        std::env::remove_var("GITHUB_TOKEN");
    }

    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());
    let response = result.unwrap();
    assert_eq!(response, "This is a mocked response from the LLM.");
}

#[tokio::test]
#[serial_test::serial]
async fn test_call_llm_github_copilot_openai_backend() {
    // Set up mock server for GitHub Copilot with OpenAI backend
    let mock_server = MockServer::start().await;
    
    // GitHub Copilot uses OpenAI-compatible API format
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer ghp-test-token"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_string(openai_mock_response())
            .insert_header("content-type", "application/json"))
        .mount(&mock_server)
        .await;

    unsafe {
        std::env::set_var("GITHUB_TOKEN", "ghp-test-token");
    }

    let config = Config {
        provider: "github-copilot/openai".to_string(),
        provider_impl: Some("openai".to_string()),
        model: "gpt-4o".to_string(),
        api_key: None,
        base_url: Some(mock_server.uri()),
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    };

    let result = call_llm(&config, "test prompt").await;

    unsafe {
        std::env::remove_var("GITHUB_TOKEN");
    }

    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());
    let response = result.unwrap();
    assert_eq!(response, "This is a mocked response from the LLM.");
}

#[test]
fn test_parse_github_copilot_backend() {
    // Test helper function to parse backend from provider string
    fn parse_github_copilot_backend(provider: &str) -> Option<&str> {
        if let Some(backend) = provider.strip_prefix("github-copilot/") {
            Some(backend)
        } else {
            None
        }
    }

    assert_eq!(
        parse_github_copilot_backend("github-copilot/anthropic"),
        Some("anthropic")
    );
    assert_eq!(
        parse_github_copilot_backend("github-copilot/openai"),
        Some("openai")
    );
    assert_eq!(parse_github_copilot_backend("openai"), None);
    assert_eq!(parse_github_copilot_backend("anthropic"), None);
}

#[tokio::test]
#[serial_test::serial]
async fn test_github_copilot_unknown_backend_error() {
    unsafe {
        std::env::set_var("GITHUB_TOKEN", "ghp-test-token");
    }

    let config = Config {
        provider: "github-copilot/unknown-backend".to_string(),
        provider_impl: Some("openai".to_string()),
        model: "some-model".to_string(),
        api_key: None,
        base_url: Some("https://api.githubcopilot.com".to_string()),
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    };

    let result = call_llm(&config, "test prompt").await;

    unsafe {
        std::env::remove_var("GITHUB_TOKEN");
    }

    // Should fail with "Unknown GitHub Copilot backend" error
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.to_string().contains("Unknown GitHub Copilot backend"));
    assert!(error.to_string().contains("unknown-backend"));
}

#[tokio::test]
#[serial_test::serial]
async fn test_github_copilot_invalid_format_error() {
    unsafe {
        std::env::set_var("GITHUB_TOKEN", "ghp-test-token");
    }

    let config = Config {
        provider: "anthropic".to_string(),  // Missing github-copilot/ prefix
        provider_impl: Some("openai".to_string()),
        model: "claude-sonnet-4.5".to_string(),
        api_key: None,
        base_url: Some("https://api.githubcopilot.com".to_string()),
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    };

    let result = call_llm(&config, "test prompt").await;

    unsafe {
        std::env::remove_var("GITHUB_TOKEN");
    }

    // Should fail with "Invalid GitHub Copilot provider format" error
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.to_string().contains("Invalid GitHub Copilot provider format"));
}

#[test]
fn test_github_copilot_backend_types_exist() {
    // Compile-time verification that backend types are properly exposed
    use crate::providers::github_copilot::{AnthropicBackend, OpenAIBackend, GitHubCopilotBackend};
    
    // Verify backend trait is implemented
    fn _assert_backend<B: GitHubCopilotBackend>(_backend: B) {}
    
    _assert_backend(AnthropicBackend);
    _assert_backend(OpenAIBackend);
    
    // Verify intent headers are different
    assert_eq!(AnthropicBackend.intent_header(), "conversation-panel");
    assert_eq!(OpenAIBackend.intent_header(), "conversation-agent");
    
    // Verify backend names
    assert_eq!(AnthropicBackend.backend_name(), "anthropic");
    assert_eq!(OpenAIBackend.backend_name(), "openai");
}
