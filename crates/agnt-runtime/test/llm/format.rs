use super::*;

use crate::config::Config;
use crate::types::{AssistantContent, CallId, Text, ToolCall, ToolFunction, ToolName};
use nu_protocol::{Span, Value};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

fn usage() -> LlmUsage {
    LlmUsage {
        input_tokens: 11,
        output_tokens: 22,
        total_tokens: 33,
        cached_input_tokens: 44,
        cache_creation_input_tokens: 55,
    }
}

fn response(text: &str) -> LlmResponse {
    LlmResponse {
        text: text.to_string(),
        usage: usage(),
        tool_calls: Vec::new(),
        tool_call_metadata: Vec::new(),
    }
}

fn config() -> Config {
    Config {
        provider: "ollama".to_string(),
        model: "llama3".to_string(),
        ..Config::default()
    }
}

/// Read a field of a `Value::Record`.
fn field<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    match value {
        Value::Record { val, .. } => val
            .get(key)
            .ok_or_else(|| format!("record must have field '{key}'").into()),
        other => Err(format!("expected a record, got {other:?}").into()),
    }
}

/// Read the elements of a `Value::List`.
fn items(value: &Value) -> Result<&[Value]> {
    match value {
        Value::List { vals, .. } => Ok(vals),
        other => Err(format!("expected a list, got {other:?}").into()),
    }
}

fn as_str(value: &Value) -> Result<&str> {
    value
        .as_str()
        .map_err(|e| format!("expected a string, got {e:?}").into())
}

fn as_int(value: &Value) -> Result<i64> {
    value
        .as_int()
        .map_err(|e| format!("expected an int, got {e:?}").into())
}

fn tool_name(name: &str) -> Result<ToolName> {
    ToolName::new(name).map_err(|e| format!("tool name must be valid: {e:?}").into())
}

// endregion: --- Test Support

/// The formatted response carries the response text, provider/model identity,
/// a timestamp, usage counters, and the session id when one is given.
#[test]
fn test_llm_format_response_identity_and_usage() -> Result<()> {
    // -- Setup & Fixtures
    let llm_response = response("hello there");
    let config = config();

    // -- Exec
    let value = format_response(&llm_response, &config, None, Span::unknown());

    // -- Check
    assert_eq!(as_str(field(&value, "response")?)?, "hello there");
    assert_eq!(as_str(field(&value, "model")?)?, "llama3");
    assert_eq!(as_str(field(&value, "provider")?)?, "ollama");
    assert!(
        !as_str(field(&value, "timestamp")?)?.is_empty(),
        "timestamp must be populated"
    );

    let meta = field(&value, "_meta")?;
    let usage = field(meta, "usage")?;
    assert_eq!(as_int(field(usage, "input_tokens")?)?, 11);
    assert_eq!(as_int(field(usage, "output_tokens")?)?, 22);
    assert_eq!(as_int(field(usage, "total_tokens")?)?, 33);
    assert_eq!(as_int(field(usage, "cached_input_tokens")?)?, 44);
    assert_eq!(as_int(field(usage, "cache_creation_input_tokens")?)?, 55);

    assert!(
        items(field(meta, "tool_calls")?)?.is_empty(),
        "a response without tool calls must report an empty tool_calls list"
    );
    assert!(
        field(meta, "session_id").is_err(),
        "session_id must be omitted when no session id is given"
    );

    // -- Exec & Check: a session id is included when present
    let value = format_response(&llm_response, &config, Some("sess-1"), Span::unknown());
    let meta = field(&value, "_meta")?;
    assert_eq!(as_str(field(meta, "session_id")?)?, "sess-1");
    Ok(())
}

/// Tool calls are sourced from `tool_call_metadata` when present, otherwise
/// from the response content; absent optional fields are omitted and
/// non-tool-call content produces no entries.
#[test]
fn test_llm_format_response_tool_calls() -> Result<()> {
    // -- Case 1: metadata entries map to tool call records
    let mut llm_response = response("calling a tool");
    llm_response.tool_call_metadata = vec![ToolCallMetadata {
        id: "call-1".to_string(),
        name: "read".to_string(),
        arguments: "{\"path\":\"/tmp/x\"}".to_string(),
        source: Some("provider".to_string()),
        error_kind: Some("invalid_args".to_string()),
        message: Some("bad path".to_string()),
        details: Some("line 3".to_string()),
    }];
    let config = config();

    let value = format_response(&llm_response, &config, None, Span::unknown());
    let calls = items(field(field(&value, "_meta")?, "tool_calls")?)?;
    assert_eq!(
        calls.len(),
        1,
        "one metadata entry must yield one tool call"
    );
    let call = calls.first().ok_or("should have one tool call")?;
    assert_eq!(as_str(field(call, "id")?)?, "call-1");
    assert_eq!(as_str(field(call, "name")?)?, "read");
    assert_eq!(as_str(field(call, "arguments")?)?, "{\"path\":\"/tmp/x\"}");
    assert_eq!(as_str(field(call, "source")?)?, "provider");
    assert_eq!(as_str(field(call, "error_kind")?)?, "invalid_args");
    assert_eq!(as_str(field(call, "message")?)?, "bad path");
    assert_eq!(as_str(field(call, "details")?)?, "line 3");

    // -- Case 2: absent optional metadata fields are omitted
    let mut llm_response = response("calling a tool");
    llm_response.tool_call_metadata = vec![ToolCallMetadata {
        id: "call-2".to_string(),
        name: "grep".to_string(),
        arguments: "{}".to_string(),
        source: None,
        error_kind: None,
        message: None,
        details: None,
    }];

    let value = format_response(&llm_response, &config, None, Span::unknown());
    let calls = items(field(field(&value, "_meta")?, "tool_calls")?)?;
    let call = calls.first().ok_or("should have one tool call")?;
    for absent in ["source", "error_kind", "message", "details"] {
        assert!(
            field(call, absent).is_err(),
            "absent optional '{absent}' must be omitted"
        );
    }

    // -- Case 3: tool calls are sourced from the response content
    let mut llm_response = response("calling a tool");
    llm_response.tool_calls = vec![AssistantContent::ToolCall(ToolCall::new(
        CallId::from_wire("call-9"),
        ToolFunction::new(tool_name("read")?, serde_json::json!({"path": "/tmp/x"})),
    ))];

    let value = format_response(&llm_response, &config, None, Span::unknown());
    let calls = items(field(field(&value, "_meta")?, "tool_calls")?)?;
    assert_eq!(calls.len(), 1, "one tool call must yield one entry");
    let call = calls.first().ok_or("should have one tool call")?;
    assert_eq!(as_str(field(call, "id")?)?, "call-9");
    assert_eq!(as_str(field(call, "name")?)?, "read");
    assert_eq!(as_str(field(call, "arguments")?)?, "{\"path\":\"/tmp/x\"}");

    // -- Case 4: non-tool-call content produces no entries
    let mut llm_response = response("just text");
    llm_response.tool_calls = vec![
        AssistantContent::Text(Text::new("not a tool call")),
        AssistantContent::text("also not a tool call"),
    ];

    let value = format_response(&llm_response, &config, None, Span::unknown());
    assert!(
        items(field(field(&value, "_meta")?, "tool_calls")?)?.is_empty(),
        "non-tool-call content must not produce tool call entries"
    );
    Ok(())
}
