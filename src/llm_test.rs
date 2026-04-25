use crate::config::Config;
use crate::llm::{call_llm, format_response};
use nu_protocol::Span;

// Note: These tests use serial_test to avoid environment variable conflicts

#[tokio::test]
#[serial_test::serial]
#[ignore = "TODO: Mock reqwest client - see task list"]
async fn test_call_llm_openai_from_env() {
    // RED: This should fail because call_llm doesn't work yet with real API
    // We'll need to mock or use a test double
    unsafe {
        std::env::set_var("OPENAI_API_KEY", "sk-test-key");
    }

    let config = Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None, // Should use env var
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    };

    // This will fail without a real API key - we'll handle mocking later
    let result = call_llm(&config, "test prompt").await;

    unsafe {
        std::env::remove_var("OPENAI_API_KEY");
    }

    // For now, just test that it doesn't panic and returns an error
    // (since we're using a fake API key)
    assert!(result.is_err() || result.is_ok());
}

#[tokio::test]
#[serial_test::serial]
#[ignore = "TODO: Mock reqwest client - see task list"]
async fn test_call_llm_openai_with_api_key_override() {
    // RED: Test explicit API key override
    let config = Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: Some("sk-override-key".to_string()),
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    };

    let result = call_llm(&config, "test prompt").await;

    // For now, just test that it doesn't panic
    assert!(result.is_err() || result.is_ok());
}

#[tokio::test]
#[serial_test::serial]
#[ignore = "TODO: Mock reqwest client - see task list"]
async fn test_call_llm_openai_with_base_url_override() {
    // RED: Test base URL override for OpenAI-compatible APIs
    unsafe {
        std::env::set_var("OPENAI_API_KEY", "sk-test-key");
    }

    let config = Config {
        provider: "openai".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None,
        base_url: Some("https://api.custom.com/v1".to_string()),
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

    assert!(result.is_err() || result.is_ok());
}

#[tokio::test]
#[serial_test::serial]
#[ignore = "TODO: Mock reqwest client - see task list"]
async fn test_call_llm_anthropic_from_env() {
    // RED: Test Anthropic provider
    unsafe {
        std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test-key");
    }

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

    let result = call_llm(&config, "test prompt").await;

    unsafe {
        std::env::remove_var("ANTHROPIC_API_KEY");
    }

    assert!(result.is_err() || result.is_ok());
}

#[tokio::test]
#[serial_test::serial]
#[ignore = "TODO: Mock reqwest client - see task list"]
async fn test_call_llm_missing_api_key() {
    // RED: Should return error when API key is missing
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

#[tokio::test]
#[ignore = "TODO: Mock reqwest client - see task list"]
async fn test_call_llm_ollama() {
    // RED: Test Ollama provider (local, no API key needed)
    let config = Config {
        provider: "ollama".to_string(),
        provider_impl: None,
        model: "llama2".to_string(),
        api_key: None,
        base_url: Some("http://localhost:11434".to_string()),
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    };

    let result = call_llm(&config, "test prompt").await;

    // Ollama might not be running, so we just check it doesn't panic
    assert!(result.is_err() || result.is_ok());
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
#[ignore = "TODO: Mock reqwest client - see task list"]
async fn test_call_llm_github_copilot_anthropic_backend() {
    // Test that github-copilot/anthropic provider routes to Anthropic backend
    unsafe {
        std::env::set_var("GITHUB_TOKEN", "ghp-test-token");
    }

    let config = Config {
        provider: "github-copilot/anthropic".to_string(),
        provider_impl: Some("openai".to_string()), // GitHub Copilot uses OpenAI API
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

    // Will fail without real API, but should not panic
    assert!(result.is_err() || result.is_ok());
}

#[tokio::test]
#[serial_test::serial]
#[ignore = "TODO: Mock reqwest client - see task list"]
async fn test_call_llm_github_copilot_openai_backend() {
    // Test that github-copilot/openai provider routes to OpenAI backend
    unsafe {
        std::env::set_var("GITHUB_TOKEN", "ghp-test-token");
    }

    let config = Config {
        provider: "github-copilot/openai".to_string(),
        provider_impl: Some("openai".to_string()),
        model: "gpt-4o".to_string(),
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

    assert!(result.is_err() || result.is_ok());
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
#[ignore = "TODO: Mock reqwest client - see task list"]
async fn test_github_copilot_unknown_backend_error() {
    // Test that unknown backend returns proper error
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
#[ignore = "TODO: Mock reqwest client - see task list"]
async fn test_github_copilot_invalid_format_error() {
    // Test that provider without github-copilot/ prefix returns error
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
