use crate::providers::github_copilot::providers::contract::GitHubCopilotProvider;
use rig::completion::request::CompletionError;

#[test]
fn concrete_provider_owns_endpoint_and_mapping_openai5x() {
    assert_eq!(
        <super::OpenAI5xProvider as GitHubCopilotProvider>::ENDPOINT_PATH,
        "/responses"
    );
    assert_eq!(
        <super::OpenAI5xProvider as GitHubCopilotProvider>::INTENT_HEADER,
        "conversation-agent"
    );
}

#[test]
fn openai5x_execute_posts_to_responses_with_valid_input_shape() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        use crate::providers::github_copilot::ClientExt;
        use rig::completion::Completion;
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::mpsc;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let (tx, rx) = mpsc::channel::<(String, String)>();

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut buf = vec![0_u8; 8192];
            let n = stream.read(&mut buf).expect("read request");
            let req = String::from_utf8_lossy(&buf[..n]).to_string();

            let header_end = req.find("\r\n\r\n").expect("header terminator");
            let headers = &req[..header_end];
            let mut lines = headers.lines();
            let request_line = lines.next().unwrap_or_default().to_string();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let lower = line.to_ascii_lowercase();
                    if lower.starts_with("content-length:") {
                        line.split(':').nth(1)?.trim().parse::<usize>().ok()
                    } else {
                        None
                    }
                })
                .unwrap_or(0);

            let mut body_bytes = req.as_bytes()[header_end + 4..].to_vec();
            while body_bytes.len() < content_length {
                let mut extra = vec![0_u8; 4096];
                let read_n = stream.read(&mut extra).expect("read body");
                if read_n == 0 {
                    break;
                }
                body_bytes.extend_from_slice(&extra[..read_n]);
            }
            body_bytes.truncate(content_length);
            let body = String::from_utf8(body_bytes).expect("utf8 body");

            tx.send((request_line, body)).expect("send captured request");

            let response_body = r#"{"id":"resp_1","model":"gpt-5.3-codex","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).expect("write response");
        });

        let base_url = format!("http://{}", addr);
        let agent = crate::providers::github_copilot::Client::agent_from_config(
            "github-copilot",
            "openai/gpt-5.3-codex",
            Some("test-token".to_string()),
            Some(base_url),
        )
        .expect("create agent");

        let crate::providers::github_copilot::Agent::OpenAI5x(agent) = agent else {
            panic!("expected OpenAI5x agent")
        };

        let _ = agent
            .completion("hello from wire test", Vec::<rig::completion::Message>::new())
            .await
            .expect("build completion")
            .tools(vec![])
            .send()
            .await
            .expect("send completion");

        let (request_line, body) = rx.recv().expect("captured request");
        handle.join().expect("server thread");

        assert!(request_line.contains("POST /responses "));
        let json: serde_json::Value = serde_json::from_str(&body).expect("json body");
        assert!(json.get("input").is_some(), "input field must exist");
        assert!(
            json.get("input").and_then(|v| v.as_str()).is_some(),
            "input must be a string for Copilot /responses compatibility"
        );
    });
}

#[test]
fn openai5x_execute_does_not_emit_chat_schema() {
    let body = serde_json::json!({
        "model": "gpt-5.3-codex",
        "input": "hello"
    });

    assert!(body.get("messages").is_none());
}

#[test]
fn openai5x_execute_error_includes_provider_and_endpoint() {
    let error = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_error(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"message":"invalid_request_body"}"#,
    );

    let msg = error.to_string();
    assert!(msg.contains("OpenAI5xProvider"));
    assert!(msg.contains("/responses"));
}

#[test]
fn map_response_supports_function_call_only_output() {
    let payload = r#"{
        "id": "resp_tool_1",
        "model": "gpt-5.3-codex",
        "status": "completed",
        "output": [
            {
                "type": "function_call",
                "id": "tool_123",
                "call_id": "call_123",
                "name": "cmd",
                "arguments": "{\"command\":\"ls\"}"
            }
        ],
        "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
    }"#;

    let mapped = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_response(payload)
        .expect("map response");

    let value = serde_json::to_value(mapped).expect("serialize mapped response");
    let tool_calls = &value["choices"][0]["message"]["tool_calls"];
    assert!(tool_calls.is_array());
    assert_eq!(tool_calls.as_array().unwrap().len(), 1);
    assert_eq!(tool_calls[0]["id"], "call_123");
    assert_eq!(tool_calls[0]["function"]["name"], "cmd");
}

// ======================================================================
// Parity tests with rig OpenAI Responses API status gating semantics
// ======================================================================

#[test]
fn parity_with_rig_openai5x_status_gate() {
    // Branch 1: Completed status → parse output → convert
    let completed_ok = r#"{"id":"r1","model":"gpt-5.3","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"test"}]}]}"#;
    let result = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_response(completed_ok);
    assert!(result.is_ok(), "completed status with content should parse");

    // Branch 2: Failed status → ProviderError with error message
    let failed = r#"{"id":"r2","model":"gpt-5.3","status":"failed","error":{"code":"rate_limit","message":"rate limit exceeded"},"output":[]}"#;
    let result = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_response(failed);
    assert!(
        matches!(result, Err(CompletionError::ProviderError(_))),
        "failed status should return ProviderError"
    );
    if let Err(CompletionError::ProviderError(msg)) = result {
        assert!(msg.contains("rate_limit"));
        assert!(msg.contains("rate limit exceeded"));
    }

    // Branch 3: Incomplete status → ProviderError with incomplete_details
    let incomplete = r#"{"id":"r3","model":"gpt-5.3","status":"incomplete","incomplete_details":{"reason":"max_tokens"},"output":[]}"#;
    let result = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_response(incomplete);
    assert!(
        matches!(result, Err(CompletionError::ProviderError(_))),
        "incomplete status should return ProviderError"
    );
    if let Err(CompletionError::ProviderError(msg)) = result {
        assert!(msg.contains("incomplete"));
        assert!(msg.contains("max_tokens"));
    }

    // Branch 4: Non-terminal status → ProviderError
    let in_progress = r#"{"id":"r4","model":"gpt-5.3","status":"in_progress","output":[]}"#;
    let result = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_response(in_progress);
    assert!(
        matches!(result, Err(CompletionError::ProviderError(_))),
        "in_progress status should return ProviderError"
    );
}

#[test]
fn test_completed_status_success_path_unchanged() {
    let payload = r#"{
        "id": "resp_ok",
        "model": "gpt-5.3",
        "status": "completed",
        "output": [
            {
                "type": "message",
                "content": [{"type": "output_text", "text": "success"}]
            }
        ]
    }"#;

    let result = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_response(payload);
    assert!(
        result.is_ok(),
        "completed status with content should succeed"
    );
}

#[test]
fn test_failed_status_returns_provider_error() {
    let payload = r#"{
        "id": "resp_fail",
        "model": "gpt-5.3",
        "status": "failed",
        "error": {"message": "model overloaded"},
        "output": []
    }"#;

    let result = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_response(payload);
    assert!(matches!(result, Err(CompletionError::ProviderError(_))));
    if let Err(CompletionError::ProviderError(msg)) = result {
        assert_eq!(msg, "model overloaded");
    }
}

#[test]
fn test_incomplete_status_returns_provider_error_with_details() {
    let payload = r#"{
        "id": "resp_incomplete",
        "model": "gpt-5.3",
        "status": "incomplete",
        "incomplete_details": {"reason": "content_filter"},
        "output": []
    }"#;

    let result = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_response(payload);
    assert!(matches!(result, Err(CompletionError::ProviderError(_))));
    if let Err(CompletionError::ProviderError(msg)) = result {
        assert!(msg.contains("incomplete"));
        assert!(msg.contains("content_filter"));
    }
}

#[test]
fn test_error_field_returns_provider_error() {
    let payload = r#"{
        "id": "resp_error",
        "model": "gpt-5.3",
        "status": "completed",
        "error": {"code": "invalid_request", "message": "bad input"},
        "output": [{"type": "message", "content": [{"type": "output_text", "text": "text"}]}]
    }"#;

    let result = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_response(payload);
    assert!(matches!(result, Err(CompletionError::ProviderError(_))));
    if let Err(CompletionError::ProviderError(msg)) = result {
        assert!(msg.contains("invalid_request"));
        assert!(msg.contains("bad input"));
    }
}

#[test]
fn test_non_terminal_status_returns_provider_error() {
    let statuses = ["in_progress", "queued", "cancelled"];
    for status in &statuses {
        let payload = format!(
            r#"{{"id":"r","model":"gpt-5.3","status":"{}","output":[]}}"#,
            status
        );
        let result = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_response(&payload);
        assert!(
            matches!(result, Err(CompletionError::ProviderError(_))),
            "status {} should return ProviderError",
            status
        );
    }
}

#[test]
fn test_empty_output_returns_response_error_like_rig() {
    let payload = r#"{
        "id": "resp_empty",
        "model": "gpt-5.3",
        "status": "completed",
        "output": []
    }"#;

    let result = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_response(payload);
    assert!(
        matches!(result, Err(CompletionError::ResponseError(_))),
        "empty output should return ResponseError like rig"
    );
    if let Err(CompletionError::ResponseError(msg)) = result {
        assert!(msg.contains("no parts"));
    }
}

#[test]
fn test_empty_content_returns_response_error_like_rig() {
    let payload = r#"{
        "id": "resp_empty_content",
        "model": "gpt-5.3",
        "status": "completed",
        "output": [{"type": "message", "content": []}]
    }"#;

    let result = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_response(payload);
    assert!(
        matches!(result, Err(CompletionError::ResponseError(_))),
        "empty content should return ResponseError like rig"
    );
    if let Err(CompletionError::ResponseError(msg)) = result {
        assert!(msg.contains("empty"));
    }
}

// ======================================================================
// Parity tests for OpenAI5x function call conversion semantics
// Source: rig-core/src/providers/openai/responses_api/mod.rs:1247-1255, 1296-1303
// ======================================================================

#[test]
fn parity_with_rig_openai5x_function_call_conversion() {
    // Test complete function_call with all required fields (id, call_id, name, arguments)
    // Mirrors rig OutputFunctionCall struct (line 1296-1303) and conversion (line 1247-1255)
    let payload = r#"{
        "id": "resp_tc",
        "model": "gpt-5.3",
        "status": "completed",
        "output": [
            {
                "type": "function_call",
                "id": "tool_001",
                "call_id": "call_abc123",
                "name": "get_weather",
                "arguments": "{\"location\":\"NYC\"}"
            }
        ]
    }"#;

    let result = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_response(payload);
    assert!(
        result.is_ok(),
        "valid function call should parse successfully"
    );

    let response = result.unwrap();
    let value = serde_json::to_value(response).expect("serialize");
    let tool_calls = &value["choices"][0]["message"]["tool_calls"];

    assert!(tool_calls.is_array());
    assert_eq!(tool_calls.as_array().unwrap().len(), 1);
    assert_eq!(tool_calls[0]["id"], "call_abc123"); // Uses call_id as tool call ID
    assert_eq!(tool_calls[0]["function"]["name"], "get_weather");
    assert_eq!(
        tool_calls[0]["function"]["arguments"],
        r#"{"location":"NYC"}"#
    );
}

#[test]
fn test_valid_args_preserved() {
    // Branch: valid JSON arguments are parsed and re-serialized
    // Source: rig json_utils::stringified_json (lines 63-72)
    let payload = r#"{
        "id": "r",
        "model": "m",
        "status": "completed",
        "output": [{
            "type": "function_call",
            "id": "t1",
            "call_id": "c1",
            "name": "cmd",
            "arguments": "{\"param\":\"value\"}"
        }]
    }"#;

    let result = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_response(payload);
    assert!(result.is_ok());

    let response = result.unwrap();
    let value = serde_json::to_value(response).unwrap();
    let args = &value["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"];
    assert_eq!(args.as_str().unwrap(), r#"{"param":"value"}"#);
}

#[test]
fn test_malformed_args_no_drop() {
    // Branch: malformed arguments JSON returns error (no tool call drop)
    // Source: rig json_utils::stringified_json returns serde error (line 71)
    // This test verifies we DON'T drop the tool call silently like old filter_map
    let payload = r#"{
        "id": "r",
        "model": "m",
        "status": "completed",
        "output": [{
            "type": "function_call",
            "id": "t1",
            "call_id": "c1",
            "name": "cmd",
            "arguments": "{malformed json"
        }]
    }"#;

    let result = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_response(payload);
    assert!(
        matches!(result, Err(CompletionError::ProviderError(_))),
        "malformed arguments should return ProviderError, not drop tool call"
    );
    if let Err(CompletionError::ProviderError(msg)) = result {
        assert!(msg.contains("Malformed"));
        assert!(msg.contains("arguments"));
    }
}

#[test]
fn test_fail_soft_representation_deterministic() {
    // Branch: empty/whitespace arguments normalize to {} (rig line 68-70)
    let test_cases = vec![
        ("", "{}"),           // empty string
        ("   ", "{}"),        // whitespace only
        ("  \\n\\t  ", "{}"), // mixed whitespace (JSON-escaped)
    ];

    for (input_args, expected_output) in test_cases {
        let payload = format!(
            r#"{{
                "id": "r",
                "model": "m",
                "status": "completed",
                "output": [{{
                    "type": "function_call",
                    "id": "t1",
                    "call_id": "c1",
                    "name": "cmd",
                    "arguments": "{}"
                }}]
            }}"#,
            input_args
        );

        let result = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_response(&payload);
        assert!(
            result.is_ok(),
            "empty/whitespace args should normalize to {{}}; input: {:?}",
            input_args
        );

        let response = result.unwrap();
        let value = serde_json::to_value(response).unwrap();
        let args = &value["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"];
        assert_eq!(
            args.as_str().unwrap(),
            expected_output,
            "args normalization mismatch for input: {:?}",
            input_args
        );
    }
}

#[test]
fn test_missing_call_linkage_returns_provider_error() {
    // Branch: missing call_id returns ProviderError (rig has call_id: String, not Option)
    // Source: rig OutputFunctionCall line 1300
    let test_cases = vec![
        (
            "missing_call_id",
            r#"{"id":"r","model":"m","status":"completed","output":[{"type":"function_call","id":"t1","name":"cmd","arguments":"{}"}]}"#,
            "call_id",
        ),
        (
            "missing_id",
            r#"{"id":"r","model":"m","status":"completed","output":[{"type":"function_call","call_id":"c1","name":"cmd","arguments":"{}"}]}"#,
            "id",
        ),
        (
            "missing_name",
            r#"{"id":"r","model":"m","status":"completed","output":[{"type":"function_call","id":"t1","call_id":"c1","arguments":"{}"}]}"#,
            "name",
        ),
    ];

    for (test_name, payload, expected_field) in test_cases {
        let result = <super::OpenAI5xProvider as GitHubCopilotProvider>::map_response(payload);
        assert!(
            matches!(result, Err(CompletionError::ProviderError(_))),
            "{}: missing {} should return ProviderError",
            test_name,
            expected_field
        );
        if let Err(CompletionError::ProviderError(msg)) = result {
            assert!(
                msg.contains(expected_field),
                "{}: error should mention field {}; got: {}",
                test_name,
                expected_field,
                msg
            );
        }
    }
}
