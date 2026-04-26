use crate::config::Config;
use crate::llm::{
    LlmResponse, LlmUsage, ProviderRoute, call_llm, format_response, parse_github_copilot_backend,
    resolve_api_key, route_provider,
};
use nu_protocol::Span;

// ============================================================================
// Helpers
// ============================================================================

fn cfg(provider: &str) -> Config {
    Config {
        provider: provider.to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    }
}

fn cfg_with_key(provider: &str, key: &str) -> Config {
    Config {
        api_key: Some(key.to_string()),
        ..cfg(provider)
    }
}

fn test_llm_response(text: &str) -> LlmResponse {
    LlmResponse {
        text: text.to_string(),
        usage: LlmUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
            cached_input_tokens: 10,
            cache_creation_input_tokens: 5,
        },
    }
}

// ============================================================================
// route_provider() tests — sync, no HTTP
// ============================================================================

#[test]
fn routes_openai_provider() {
    assert_eq!(route_provider(&cfg("openai")), ProviderRoute::OpenAI);
}

#[test]
fn routes_anthropic_provider() {
    assert_eq!(route_provider(&cfg("anthropic")), ProviderRoute::Anthropic);
}

#[test]
fn routes_ollama_provider() {
    assert_eq!(route_provider(&cfg("ollama")), ProviderRoute::Ollama);
}

#[test]
fn routes_github_copilot_anthropic() {
    let route = route_provider(&cfg("github-copilot/anthropic"));
    assert_eq!(
        route,
        ProviderRoute::GitHubCopilot {
            backend: "anthropic".to_string()
        }
    );
}

#[test]
fn routes_github_copilot_openai() {
    let route = route_provider(&cfg("github-copilot/openai"));
    assert_eq!(
        route,
        ProviderRoute::GitHubCopilot {
            backend: "openai".to_string()
        }
    );
}

#[test]
fn routes_unsupported_provider() {
    let route = route_provider(&cfg("groq"));
    assert_eq!(route, ProviderRoute::Unsupported("groq".to_string()));
}

#[test]
fn routes_github_copilot_via_legacy_base_url() {
    let config = Config {
        base_url: Some("https://api.githubcopilot.com/v1".to_string()),
        ..cfg("openai")
    };
    assert!(matches!(
        route_provider(&config),
        ProviderRoute::GitHubCopilot { .. }
    ));
}

#[test]
fn routes_using_provider_impl_over_provider() {
    let config = Config {
        provider_impl: Some("openai".to_string()),
        ..cfg("my-custom")
    };
    assert_eq!(route_provider(&config), ProviderRoute::OpenAI);
}

// ============================================================================
// resolve_api_key() tests — sync, no HTTP
// ============================================================================

#[test]
fn returns_config_api_key_when_set() {
    let config = cfg_with_key("openai", "sk-explicit");
    let result = resolve_api_key(&config, "openai");
    assert_eq!(result.unwrap(), "sk-explicit");
}

#[test]
#[serial_test::serial]
fn returns_openai_env_var_when_config_key_is_none() {
    unsafe { std::env::set_var("OPENAI_API_KEY", "sk-from-env") };
    let result = resolve_api_key(&cfg("openai"), "openai");
    unsafe { std::env::remove_var("OPENAI_API_KEY") };
    assert_eq!(result.unwrap(), "sk-from-env");
}

#[test]
#[serial_test::serial]
fn returns_anthropic_env_var_when_config_key_is_none() {
    unsafe { std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-env") };
    let result = resolve_api_key(&cfg("anthropic"), "anthropic");
    unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };
    assert_eq!(result.unwrap(), "sk-ant-env");
}

#[test]
#[serial_test::serial]
fn uses_github_token_for_github_copilot_provider() {
    unsafe { std::env::set_var("GITHUB_TOKEN", "ghp-xyz") };
    let result = resolve_api_key(&cfg("github-copilot"), "github-copilot");
    unsafe { std::env::remove_var("GITHUB_TOKEN") };
    assert_eq!(result.unwrap(), "ghp-xyz");
}

#[test]
#[serial_test::serial]
fn does_not_read_generated_github_copilot_api_key_env_var() {
    // Ensure GITHUB_TOKEN is not set; set the wrong var name
    unsafe { std::env::remove_var("GITHUB_TOKEN") };
    unsafe { std::env::set_var("GITHUB-COPILOT_API_KEY", "should-not-read") };
    let result = resolve_api_key(&cfg("github-copilot"), "github-copilot");
    unsafe { std::env::remove_var("GITHUB-COPILOT_API_KEY") };
    assert!(
        result.is_err(),
        "Should error when only wrong env var is set"
    );
}

#[test]
#[serial_test::serial]
fn returns_err_when_no_config_key_and_no_env_var() {
    unsafe { std::env::remove_var("OPENAI_API_KEY") };
    let result = resolve_api_key(&cfg("openai"), "openai");
    assert!(result.is_err());
    assert!(result.unwrap_err().msg.contains("Missing API key"));
}

#[test]
#[serial_test::serial]
fn config_api_key_takes_precedence_over_env() {
    unsafe { std::env::set_var("OPENAI_API_KEY", "env-key") };
    let config = cfg_with_key("openai", "config-key");
    let result = resolve_api_key(&config, "openai");
    unsafe { std::env::remove_var("OPENAI_API_KEY") };
    assert_eq!(result.unwrap(), "config-key");
}

// ============================================================================
// parse_github_copilot_backend() tests — sync, no HTTP
// ============================================================================

#[test]
fn parses_anthropic_backend() {
    assert_eq!(
        parse_github_copilot_backend("github-copilot/anthropic").unwrap(),
        "anthropic"
    );
}

#[test]
fn parses_openai_backend() {
    assert_eq!(
        parse_github_copilot_backend("github-copilot/openai").unwrap(),
        "openai"
    );
}

#[test]
fn rejects_provider_string_without_github_copilot_prefix() {
    let result = parse_github_copilot_backend("anthropic");
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .msg
            .contains("Invalid GitHub Copilot provider format")
    );
}

#[test]
fn rejects_empty_backend_after_prefix() {
    let result = parse_github_copilot_backend("github-copilot/");
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .msg
            .contains("Invalid GitHub Copilot provider format")
    );
}

// ============================================================================
// call_llm() error-path tests — async but NO HTTP (fail before rig client)
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_call_llm_missing_api_key() {
    unsafe { std::env::remove_var("OPENAI_API_KEY") };

    let result = call_llm(&cfg("openai"), "test prompt").await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(
        err.msg.contains("Missing API key") || err.msg.contains("OPENAI_API_KEY"),
        "Unexpected error: {}",
        err.msg
    );
}

#[tokio::test]
async fn test_call_llm_unsupported_provider() {
    let config = cfg_with_key("unsupported", "key");
    let result = call_llm(&config, "test prompt").await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(err.msg.contains("Unsupported provider"));
}

#[tokio::test]
#[serial_test::serial]
async fn call_llm_returns_err_for_missing_anthropic_key() {
    unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };

    let result = call_llm(&cfg("anthropic"), "test prompt").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().msg.contains("Missing API key"));
}

#[tokio::test]
#[serial_test::serial]
async fn call_llm_returns_err_for_missing_github_token() {
    unsafe { std::env::remove_var("GITHUB_TOKEN") };

    let result = call_llm(&cfg("github-copilot/anthropic"), "test prompt").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().msg.contains("Missing API key"));
}

#[tokio::test]
#[ignore]
async fn call_llm_returns_err_for_unknown_github_copilot_backend() {
    let config = cfg_with_key("github-copilot/foobar", "ghp-key");
    let result = call_llm(&config, "test prompt").await;
    assert!(result.is_err());
    let msg = result.unwrap_err().msg;
    assert!(
        msg.contains("Unknown GitHub Copilot backend"),
        "Unexpected error: {}",
        msg
    );
    assert!(
        msg.contains("foobar"),
        "Backend name missing from error: {}",
        msg
    );
}

// ============================================================================
// format_response() tests — sync, pure function
// ============================================================================

#[test]
fn test_format_response_basic() {
    let config = Config {
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        ..cfg("openai")
    };

    let response_text = "This is a test response from the LLM.";
    let llm_response = test_llm_response(response_text);
    let value = format_response(&llm_response, &config, None, 0, Span::unknown());

    let record = value.as_record().expect("Should be a record");

    assert!(record.contains("response"));
    assert!(record.contains("model"));
    assert!(record.contains("provider"));
    assert!(record.contains("timestamp"));

    assert_eq!(
        record.get("response").unwrap().as_str().unwrap(),
        response_text
    );
    assert_eq!(record.get("model").unwrap().as_str().unwrap(), "gpt-4");
    assert_eq!(record.get("provider").unwrap().as_str().unwrap(), "openai");

    let timestamp = record.get("timestamp").unwrap().as_str().unwrap();
    assert!(timestamp.contains('T'));
    assert!(timestamp.contains('Z') || timestamp.contains('+') || timestamp.contains('-'));
}

#[test]
fn test_format_response_empty() {
    let config = Config {
        provider: "anthropic".to_string(),
        model: "claude-3-opus".to_string(),
        ..cfg("anthropic")
    };

    let llm_response = test_llm_response("");
    let value = format_response(&llm_response, &config, None, 0, Span::unknown());
    let record = value.as_record().expect("Should be a record");
    assert_eq!(record.get("response").unwrap().as_str().unwrap(), "");
}

#[test]
fn test_format_response_includes_meta_field() {
    let config = Config {
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        ..cfg("openai")
    };

    let llm_response = test_llm_response("Test response");
    let value = format_response(&llm_response, &config, None, 0, Span::unknown());

    let record = value.as_record().expect("Should be a record");

    // Verify _meta field exists
    assert!(
        record.contains("_meta"),
        "_meta field should exist in response record"
    );

    // Verify _meta is a record
    let meta = record.get("_meta").expect("_meta field should exist");
    let meta_record = meta.as_record().expect("_meta should be a record");

    // Verify _meta contains required fields (session_id is optional)
    assert!(
        meta_record.contains("compacted"),
        "_meta should contain compacted"
    );
    assert!(
        meta_record.contains("compaction_count"),
        "_meta should contain compaction_count"
    );
    assert!(
        meta_record.contains("tool_calls"),
        "_meta should contain tool_calls"
    );
    assert!(meta_record.contains("usage"), "_meta should contain usage");

    // Verify usage record exists and has correct fields
    let usage = meta_record.get("usage").unwrap();
    let usage_record = usage.as_record().expect("usage should be a record");

    assert_eq!(
        usage_record.get("input_tokens").unwrap().as_int().unwrap(),
        100
    );
    assert_eq!(
        usage_record.get("output_tokens").unwrap().as_int().unwrap(),
        50
    );
    assert_eq!(
        usage_record.get("total_tokens").unwrap().as_int().unwrap(),
        150
    );
    assert_eq!(
        usage_record
            .get("cached_input_tokens")
            .unwrap()
            .as_int()
            .unwrap(),
        10
    );
    assert_eq!(
        usage_record
            .get("cache_creation_input_tokens")
            .unwrap()
            .as_int()
            .unwrap(),
        5
    );

    // Verify default values when no session_id is provided
    assert!(
        meta_record.get("session_id").is_none(),
        "session_id should not be present when None is passed"
    );
    assert!(
        !meta_record.get("compacted").unwrap().as_bool().unwrap(),
        "compacted should be false when compaction_count is 0"
    );
    assert_eq!(
        meta_record
            .get("compaction_count")
            .unwrap()
            .as_int()
            .unwrap(),
        0,
        "compaction_count should be 0 when passed as 0"
    );

    // Verify tool_calls is an empty list
    let tool_calls = meta_record.get("tool_calls").unwrap();
    assert!(tool_calls.as_list().is_ok(), "tool_calls should be a list");
    assert_eq!(
        tool_calls.as_list().unwrap().len(),
        0,
        "tool_calls should be empty by default"
    );
}

#[test]
fn test_format_response_with_session_metadata() {
    let config = Config {
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        ..cfg("openai")
    };

    let llm_response = test_llm_response("Test response");
    let session_id = Some("abc123de");
    let compaction_count = 3;
    let value = format_response(
        &llm_response,
        &config,
        session_id,
        compaction_count,
        Span::unknown(),
    );

    let record = value.as_record().expect("Should be a record");
    let meta = record.get("_meta").expect("_meta field should exist");
    let meta_record = meta.as_record().expect("_meta should be a record");

    // Verify session_id is present
    assert_eq!(
        meta_record.get("session_id").unwrap().as_str().unwrap(),
        "abc123de",
        "session_id should match provided value"
    );

    // Verify compacted is true when compaction_count > 0
    assert!(
        meta_record.get("compacted").unwrap().as_bool().unwrap(),
        "compacted should be true when compaction_count > 0"
    );

    // Verify compaction_count matches
    assert_eq!(
        meta_record
            .get("compaction_count")
            .unwrap()
            .as_int()
            .unwrap(),
        3,
        "compaction_count should match provided value"
    );
}

// ============================================================================
// Compile-time verification: backend types are properly exposed
// ============================================================================

#[test]
fn github_copilot_backend_types_exist() {
    use crate::providers::github_copilot::{AnthropicBackend, GitHubCopilotBackend, OpenAIBackend};

    fn assert_backend<B: GitHubCopilotBackend>(_backend: B) {}

    assert_backend(AnthropicBackend);
    assert_backend(OpenAIBackend);

    assert_eq!(AnthropicBackend.intent_header(), "conversation-panel");
    assert_eq!(OpenAIBackend.intent_header(), "conversation-agent");

    assert_eq!(AnthropicBackend.backend_name(), "anthropic");
    assert_eq!(OpenAIBackend.backend_name(), "openai");
}

// ============================================================================
// LlmUsage and LlmResponse tests — pure data structures
// ============================================================================

#[test]
fn llm_usage_struct_has_all_fields() {
    use crate::llm::LlmUsage;

    let usage = LlmUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
        cached_input_tokens: 20,
        cache_creation_input_tokens: 10,
    };

    assert_eq!(usage.input_tokens, 100);
    assert_eq!(usage.output_tokens, 50);
    assert_eq!(usage.total_tokens, 150);
    assert_eq!(usage.cached_input_tokens, 20);
    assert_eq!(usage.cache_creation_input_tokens, 10);
}

#[test]
fn llm_response_struct_has_text_and_usage() {
    use crate::llm::{LlmResponse, LlmUsage};

    let usage = LlmUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
        cached_input_tokens: 20,
        cache_creation_input_tokens: 10,
    };

    let response = LlmResponse {
        text: "Hello, world!".to_string(),
        usage,
    };

    assert_eq!(response.text, "Hello, world!");
    assert_eq!(response.usage.input_tokens, 100);
    assert_eq!(response.usage.output_tokens, 50);
}

#[test]
fn llm_usage_converts_from_rig_usage() {
    use crate::llm::LlmUsage;
    use rig::completion::request::Usage as RigUsage;

    let rig_usage = RigUsage {
        input_tokens: 200,
        output_tokens: 75,
        total_tokens: 275,
        cached_input_tokens: 30,
        cache_creation_input_tokens: 15,
    };

    let llm_usage: LlmUsage = rig_usage.into();

    assert_eq!(llm_usage.input_tokens, 200);
    assert_eq!(llm_usage.output_tokens, 75);
    assert_eq!(llm_usage.total_tokens, 275);
    assert_eq!(llm_usage.cached_input_tokens, 30);
    assert_eq!(llm_usage.cache_creation_input_tokens, 15);
}

// ============================================================================
// call_llm() return type tests - verifying LlmResponse
// ============================================================================

// Note: These tests verify that call_llm() *would* return LlmResponse,
// but we can't actually test the async implementation without real API calls.
// We verify the signature change compiles and the error path works.

#[tokio::test]
#[serial_test::serial]
async fn test_call_llm_would_return_llm_response_not_string() {
    // This test verifies the signature changed from Result<String, _> to Result<LlmResponse, _>
    // We can only test error cases without real API calls
    unsafe { std::env::remove_var("OPENAI_API_KEY") };

    let result = call_llm(&cfg("openai"), "test prompt").await;

    // This should be Err since no API key
    assert!(result.is_err());

    // If it were Ok, the type would be LlmResponse (compile-time check)
    // Uncomment to verify compile error if Result<String> was returned:
    // if let Ok(response) = result {
    //     let _usage = response.usage;  // Would fail if response was String
    // }
}
