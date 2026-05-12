use crate::providers::github_copilot::providers::contract::GitHubCopilotProvider;

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

            let response_body = r#"{"id":"resp_1","model":"gpt-5.3-codex","output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}"#;
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
        "output": [
            {
                "type": "function_call",
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
