use crate::config::Config;
use crate::llm::{LlmResponse, LlmUsage, call_llm, extract_response, format_response};
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
        tool_calls: vec![],
    }
}

// ============================================================================
// call_llm() error-path tests — async but NO HTTP (fail before rig client)
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_call_llm_missing_api_key() {
    unsafe { std::env::remove_var("OPENAI_API_KEY") };

    let result = call_llm(&cfg("openai"), "test prompt", vec![]).await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(
        err.msg.contains("Missing") || err.msg.contains("OPENAI_API_KEY"),
        "Expected missing API key error, got: {}",
        err.msg
    );
}

#[tokio::test]
async fn test_call_llm_unsupported_provider() {
    let config = cfg_with_key("unsupported", "key");
    let result = call_llm(&config, "test prompt", vec![]).await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(err.msg.contains("Unsupported provider"));
}

#[tokio::test]
#[serial_test::serial]
async fn call_llm_returns_err_for_missing_anthropic_key() {
    unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };

    let result = call_llm(&cfg("anthropic"), "test prompt", vec![]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.msg.contains("Missing") || err.msg.contains("ANTHROPIC_API_KEY"),
        "Expected missing API key error, got: {}",
        err.msg
    );
}

#[tokio::test]
#[serial_test::serial]
async fn call_llm_returns_err_for_missing_github_token() {
    unsafe { std::env::remove_var("GITHUB_TOKEN") };

    // Create config with new format: provider = "github-copilot", model = "anthropic/model"
    let config = Config {
        provider: "github-copilot".to_string(),
        model: "anthropic/claude-sonnet-4.5".to_string(),
        ..cfg("github-copilot")
    };

    let result = call_llm(&config, "test prompt", vec![]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.msg.contains("Missing")
            || err.msg.contains("API key")
            || err.msg.contains("GITHUB_TOKEN"),
        "Expected missing API key error, got: {}",
        err.msg
    );
}

#[tokio::test]
#[ignore]
async fn call_llm_returns_err_for_unknown_github_copilot_backend() {
    let config = Config {
        provider: "github-copilot".to_string(),
        model: "foobar/some-model".to_string(),
        api_key: Some("ghp-key".to_string()),
        ..cfg("github-copilot")
    };
    let result = call_llm(&config, "test prompt", vec![]).await;
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
        tool_calls: vec![],
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

    let result = call_llm(&cfg("openai"), "test prompt", vec![]).await;

    // This should be Err since no API key
    assert!(result.is_err());

    // If it were Ok, the type would be LlmResponse (compile-time check)
    // Uncomment to verify compile error if Result<String> was returned:
    // if let Ok(response) = result {
    //     let _usage = response.usage;  // Would fail if response was String
    // }
}

// ============================================================================
// Tool definitions parameter tests
// ============================================================================

#[tokio::test]
async fn call_llm_accepts_empty_tool_definitions() {
    // RED: Test that call_llm accepts tools parameter (even if empty)
    // This should compile and not fail due to empty tools
    let tools = vec![];
    let result = call_llm(&cfg("openai"), "test prompt", tools).await;

    // Expected to fail due to no API key, but function signature should accept tools
    assert!(result.is_err());
}

#[tokio::test]
async fn call_llm_accepts_tool_definitions() {
    // RED: Test that call_llm accepts actual tool definitions
    use rig::completion::ToolDefinition;
    use serde_json::json;

    let tools = vec![ToolDefinition {
        name: "add".to_string(),
        description: "Add two numbers".to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "args": {
                    "type": "array",
                    "items": {}
                }
            },
            "required": ["args"]
        }),
    }];

    let result = call_llm(&cfg("openai"), "test prompt", tools).await;

    // Expected to fail due to no API key, but function should accept tools
    assert!(result.is_err());
}

// ============================================================================
// extract_response() tests — behavioral tests
// ============================================================================

#[test]
fn extract_response_handles_text_and_tool_calls() {
    use rig::OneOrMany;
    use rig::completion::CompletionResponse;
    use rig::completion::message::{AssistantContent, Text, ToolCall, ToolFunction};
    use rig::completion::request::Usage;
    use serde_json::json;

    // Create mock completion response with text and tool calls
    let text1 = AssistantContent::Text(Text {
        text: "First line".to_string(),
    });
    let text2 = AssistantContent::Text(Text {
        text: "Second line".to_string(),
    });
    let tool_call = AssistantContent::ToolCall(ToolCall::new(
        "call_123".to_string(),
        ToolFunction::new("test_tool".to_string(), json!({"arg": "value"})),
    ));

    // Create CompletionResponse with the content
    let completion_response = CompletionResponse::<()> {
        choice: OneOrMany::many(vec![text1, tool_call.clone(), text2])
            .expect("Should create OneOrMany"),
        usage: Usage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
            cached_input_tokens: 10,
            cache_creation_input_tokens: 5,
        },
        raw_response: (),
        message_id: None,
    };

    let result = extract_response(completion_response).expect("Should extract successfully");

    // Verify text parts are joined with \n
    assert_eq!(result.text, "First line\nSecond line");

    // Verify usage is converted correctly
    assert_eq!(result.usage.input_tokens, 100);
    assert_eq!(result.usage.output_tokens, 50);
    assert_eq!(result.usage.total_tokens, 150);
    assert_eq!(result.usage.cached_input_tokens, 10);
    assert_eq!(result.usage.cache_creation_input_tokens, 5);

    // Verify tool calls are extracted
    assert_eq!(result.tool_calls.len(), 1);
    if let AssistantContent::ToolCall(tc) = &result.tool_calls[0] {
        assert_eq!(tc.id, "call_123");
        assert_eq!(tc.function.name, "test_tool");
    } else {
        panic!("Expected ToolCall variant");
    }
}

#[test]
fn extract_response_ignores_other_content_types() {
    use rig::OneOrMany;
    use rig::completion::CompletionResponse;
    use rig::completion::message::{AssistantContent, Reasoning, Text};
    use rig::completion::request::Usage;

    // Create mock completion response with text and reasoning (other types to ignore)
    let text = AssistantContent::Text(Text {
        text: "Only this should be extracted".to_string(),
    });
    let reasoning = AssistantContent::Reasoning(Reasoning::new("This should be ignored"));

    let completion_response = CompletionResponse::<()> {
        choice: OneOrMany::many(vec![reasoning, text]).expect("Should create OneOrMany"),
        usage: Usage {
            input_tokens: 50,
            output_tokens: 25,
            total_tokens: 75,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
        },
        raw_response: (),
        message_id: None,
    };

    let result = extract_response(completion_response).expect("Should extract successfully");

    // Verify only text is extracted, reasoning is ignored
    assert_eq!(result.text, "Only this should be extracted");

    // Verify no tool calls
    assert_eq!(result.tool_calls.len(), 0);
}

#[test]
fn extract_response_handles_empty_choice() {
    use rig::OneOrMany;
    use rig::completion::CompletionResponse;
    use rig::completion::message::{AssistantContent, Text};
    use rig::completion::request::Usage;

    // Create mock completion response with single empty text (OneOrMany requires at least one item)
    let empty_text = AssistantContent::Text(Text {
        text: "".to_string(),
    });

    let completion_response = CompletionResponse::<()> {
        choice: OneOrMany::one(empty_text),
        usage: Usage {
            input_tokens: 10,
            output_tokens: 0,
            total_tokens: 10,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
        },
        raw_response: (),
        message_id: None,
    };

    let result = extract_response(completion_response).expect("Should extract successfully");

    // Verify empty text
    assert_eq!(result.text, "");

    // Verify no tool calls
    assert_eq!(result.tool_calls.len(), 0);

    // Verify usage is still converted
    assert_eq!(result.usage.input_tokens, 10);
    assert_eq!(result.usage.output_tokens, 0);
}

#[test]
fn extract_response_handles_multiple_text_parts() {
    use rig::OneOrMany;
    use rig::completion::CompletionResponse;
    use rig::completion::message::{AssistantContent, Text};
    use rig::completion::request::Usage;

    // Create mock completion response with multiple text parts
    let text1 = AssistantContent::Text(Text {
        text: "Line 1".to_string(),
    });
    let text2 = AssistantContent::Text(Text {
        text: "Line 2".to_string(),
    });
    let text3 = AssistantContent::Text(Text {
        text: "Line 3".to_string(),
    });

    let completion_response = CompletionResponse::<()> {
        choice: OneOrMany::many(vec![text1, text2, text3]).expect("Should create OneOrMany"),
        usage: Usage {
            input_tokens: 30,
            output_tokens: 15,
            total_tokens: 45,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
        },
        raw_response: (),
        message_id: None,
    };

    let result = extract_response(completion_response).expect("Should extract successfully");

    // Verify text parts are joined with newlines
    assert_eq!(result.text, "Line 1\nLine 2\nLine 3");

    // Verify no tool calls
    assert_eq!(result.tool_calls.len(), 0);
}
