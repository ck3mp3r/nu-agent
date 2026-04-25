use crate::config::Config;
use crate::llm::{
    call_llm, format_response, parse_github_copilot_backend, resolve_api_key, route_provider,
    ProviderRoute,
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
    assert!(result.is_err(), "Should error when only wrong env var is set");
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
    assert!(result
        .unwrap_err()
        .msg
        .contains("Invalid GitHub Copilot provider format"));
}

#[test]
fn rejects_empty_backend_after_prefix() {
    let result = parse_github_copilot_backend("github-copilot/");
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .msg
        .contains("Invalid GitHub Copilot provider format"));
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
    assert!(msg.contains("foobar"), "Backend name missing from error: {}", msg);
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

    let response = "This is a test response from the LLM.";
    let value = format_response(response, &config, Span::unknown());

    let record = value.as_record().expect("Should be a record");

    assert!(record.contains("response"));
    assert!(record.contains("model"));
    assert!(record.contains("provider"));
    assert!(record.contains("timestamp"));

    assert_eq!(record.get("response").unwrap().as_str().unwrap(), response);
    assert_eq!(record.get("model").unwrap().as_str().unwrap(), "gpt-4");
    assert_eq!(record.get("provider").unwrap().as_str().unwrap(), "openai");

    let timestamp = record.get("timestamp").unwrap().as_str().unwrap();
    assert!(timestamp.contains('T'));
    assert!(
        timestamp.contains('Z') || timestamp.contains('+') || timestamp.contains('-')
    );
}

#[test]
fn test_format_response_empty() {
    let config = Config {
        provider: "anthropic".to_string(),
        model: "claude-3-opus".to_string(),
        ..cfg("anthropic")
    };

    let value = format_response("", &config, Span::unknown());
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

    let response = "Test response";
    let value = format_response(response, &config, Span::unknown());

    let record = value.as_record().expect("Should be a record");

    // Verify _meta field exists
    assert!(
        record.contains("_meta"),
        "_meta field should exist in response record"
    );

    // Verify _meta is a record
    let meta = record.get("_meta").expect("_meta field should exist");
    let meta_record = meta
        .as_record()
        .expect("_meta should be a record");

    // Verify _meta contains required fields
    assert!(
        meta_record.contains("session_id"),
        "_meta should contain session_id"
    );
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

    // Verify default values (placeholders for now)
    assert_eq!(
        meta_record.get("session_id").unwrap().as_str().unwrap(),
        "temp",
        "session_id should default to 'temp'"
    );
    assert_eq!(
        meta_record.get("compacted").unwrap().as_bool().unwrap(),
        false,
        "compacted should default to false"
    );
    assert_eq!(
        meta_record.get("compaction_count").unwrap().as_int().unwrap(),
        0,
        "compaction_count should default to 0"
    );

    // Verify tool_calls is an empty list
    let tool_calls = meta_record.get("tool_calls").unwrap();
    assert!(
        tool_calls.as_list().is_ok(),
        "tool_calls should be a list"
    );
    assert_eq!(
        tool_calls.as_list().unwrap().len(),
        0,
        "tool_calls should be empty by default"
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
