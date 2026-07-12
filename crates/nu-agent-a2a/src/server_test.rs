use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use super::*;

// The workspace reqwest is built with `rustls-no-provider`, meaning the
// application must install a crypto provider before constructing a Client.
static CRYPTO_INIT: std::sync::Once = std::sync::Once::new();

fn ensure_crypto_provider() {
    CRYPTO_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Create a [`reqwest::Client`] that sends `A2A-Version: 1.0` on every
/// request, matching what the middleware expects on A2A API paths.
fn test_client() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::HeaderName::from_static("a2a-version"),
        reqwest::header::HeaderValue::from_static(crate::A2A_VERSION),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap()
}

// ---------------------------------------------------------------------------
// A2aServer
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_server_starts_and_returns_port() {
    ensure_crypto_provider();

    let card = AgentCard {
        name: "test".to_string(),
        url: "http://127.0.0.1:0".to_string(),
        ..Default::default()
    };
    let server = A2aServer::start(card, Arc::new(PeerCache::new()))
        .await
        .unwrap();
    assert!(server.port > 0, "Port should be > 0");
    assert_eq!(
        server.local_url,
        format!("http://127.0.0.1:{}", server.port)
    );

    // Health endpoint responds
    let resp = reqwest::get(&format!("{}/health", server.local_url))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    assert_eq!(body, "ok");

    server.shutdown().await;
}

#[tokio::test]
async fn test_a2a_version_response_header() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Health endpoint
    let resp = client
        .get(format!("{}/health", server.local_url))
        .send()
        .await
        .unwrap();
    let version = resp
        .headers()
        .get("A2A-Version")
        .and_then(|v| v.to_str().ok());
    assert_eq!(
        version,
        Some("1.0"),
        "health response should include A2A-Version header"
    );

    // A2A API endpoint
    let resp = client
        .post(format!("{}/tasks/list", server.local_url))
        .json(&serde_json::json!({"id": "test"}))
        .send()
        .await
        .unwrap();
    let version = resp
        .headers()
        .get("A2A-Version")
        .and_then(|v| v.to_str().ok());
    assert_eq!(
        version,
        Some("1.0"),
        "A2A API response should include A2A-Version header"
    );

    // agent.json endpoint
    let resp = client
        .get(format!("{}/agent.json", server.local_url))
        .send()
        .await
        .unwrap();
    let version = resp
        .headers()
        .get("A2A-Version")
        .and_then(|v| v.to_str().ok());
    assert_eq!(
        version,
        Some("1.0"),
        "agent.json response should include A2A-Version header"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_server_cleanup_frees_port() {
    ensure_crypto_provider();

    let card = AgentCard {
        name: "cleanup-test".to_string(),
        url: "http://127.0.0.1:0".to_string(),
        ..Default::default()
    };
    let server = A2aServer::start(card, Arc::new(PeerCache::new()))
        .await
        .unwrap();
    let port = server.port;

    server.shutdown().await;

    // Give the OS time to release the TCP port from TIME_WAIT state.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Should be able to bind to same port now
    tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .expect("Port should be free after shutdown");
}

// ---------------------------------------------------------------------------
// Route handler integration tests (against real running server)
// ---------------------------------------------------------------------------

async fn test_server() -> (A2aServer, reqwest::Client) {
    let card = AgentCard {
        name: "test-agent".into(),
        url: "http://127.0.0.1:0".into(),
        version: "1.0".into(),
        capabilities: AgentCapabilities::default(),
        skills: vec![Skill {
            id: "test-skill".into(),
            name: "Test".into(),
            description: "A test skill".into(),
            inputs: None,
            outputs: None,
        }],
        ..Default::default()
    };
    let server = A2aServer::start(card, Arc::new(PeerCache::new()))
        .await
        .unwrap();
    let client = test_client();
    (server, client)
}

#[tokio::test]
async fn test_agent_card_endpoint() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .get(format!("{}/agent.json", server.local_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "test-agent");
    assert_eq!(body["skills"].as_array().unwrap().len(), 1);
    assert_eq!(body["skills"][0]["name"], "Test");

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_send_creates_task() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({
            "id": "req-1",
            "message": {
                "role": "user",
                "parts": [{"type": "text", "text": "hello"}]
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["id"], "req-1");
    assert!(body.get("result").is_some(), "Should have a result");
    assert!(
        !body["result"]["id"].as_str().unwrap().is_empty(),
        "Task should have an ID"
    );
    assert_eq!(body["result"]["status"]["state"], "working");

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_send_missing_message() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({"id": "req-1"}))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("error").is_some(),
        "Should return error for missing message"
    );
    assert_eq!(body["error"]["code"], -32602);

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_get_returns_task() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task first
    let send_resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({"id": "req-1", "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["result"]["id"].as_str().unwrap().to_string();

    // Get the task
    let resp = client
        .get(format!("{}/tasks/{}", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["result"]["id"], task_id);
    assert_eq!(body["result"]["status"]["state"], "working");

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_get_not_found() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .get(format!("{}/tasks/nonexistent-id", server.local_url))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("error").is_some());
    assert_eq!(body["error"]["code"], -32001);

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_cancel() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create task
    let send_resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({"id": "req-1", "message": {"role": "user", "parts": [{"type": "text", "text": "cancel me"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["result"]["id"].as_str().unwrap().to_string();

    // Cancel it
    let resp = client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["result"]["status"]["state"], "canceled");

    server.shutdown().await;
}

#[tokio::test]
async fn test_cancel_completed_fails() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let send_resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({"id": "req-1", "message": {"role": "user", "parts": [{"type": "text", "text": "done"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["result"]["id"].as_str().unwrap().to_string();

    // First cancel should succeed
    let _ = client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    // Second cancel should fail (already canceled)
    let resp = client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("error").is_some(), "Second cancel should fail");
    assert_eq!(body["error"]["code"], -32000);

    server.shutdown().await;
}

#[tokio::test]
async fn test_task_lifecycle_full() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create
    let send_resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({"id": "req-1", "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["result"]["id"].as_str().unwrap().to_string();
    assert_eq!(send_body["result"]["status"]["state"], "working");

    // Get
    let get_resp = client
        .get(format!("{}/tasks/{}", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    let get_body: serde_json::Value = get_resp.json().await.unwrap();
    assert_eq!(get_body["result"]["status"]["state"], "working");

    // Cancel
    let cancel_resp = client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    let cancel_body: serde_json::Value = cancel_resp.json().await.unwrap();
    assert_eq!(cancel_body["result"]["status"]["state"], "canceled");

    // Get after cancel
    let get2_resp = client
        .get(format!("{}/tasks/{}", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    let get2_body: serde_json::Value = get2_resp.json().await.unwrap();
    assert_eq!(get2_body["result"]["status"]["state"], "canceled");

    server.shutdown().await;
}

#[tokio::test]
async fn test_concurrent_requests() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;
    let url = server.local_url.clone();
    let mut handles = vec![];

    for i in 0..5 {
        let c = client.clone();
        let u = url.clone();
        handles.push(tokio::spawn(async move {
            c.post(format!("{u}/tasks/send"))
                .json(&json!({"id": format!("req-{i}"), "message": {"role": "user", "parts": [{"type": "text", "text": format!("msg-{i}")}]}}))
                .send()
                .await
                .unwrap()
        }));
    }

    for h in handles {
        let resp = h.await.unwrap();
        assert_eq!(resp.status(), 200);
    }

    server.shutdown().await;
}

#[tokio::test]
async fn test_send_stream_returns_sse() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/tasks/sendStream", server.local_url))
        .json(&json!({"id": "req-1", "message": {"role": "user", "parts": [{"type": "text", "text": "stream"}]}}))
        .send()
        .await
        .unwrap();

    // Verify SSE content type
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/event-stream"),
        "Expected SSE content type, got: {content_type}"
    );

    // Give the server a moment to create the task
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Look up the task via list to get its ID
    let list_resp = client
        .post(format!("{}/tasks/list", server.local_url))
        .json(&json!({"id": "list-1"}))
        .send()
        .await
        .unwrap();
    let list_body: serde_json::Value = list_resp.json().await.unwrap();
    let tasks = list_body["result"]["tasks"].as_array().unwrap();
    let task_id = tasks
        .iter()
        .find(|t| t["status"]["state"] == "working")
        .and_then(|t| t["id"].as_str())
        .expect("should find a working task");

    // Cancel the task to trigger a terminal event and close the SSE stream
    client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    // Read the SSE body (stream should close after the cancel event)
    tokio::time::sleep(Duration::from_millis(200)).await;
    let body = resp.bytes().await.unwrap();
    let text = String::from_utf8_lossy(&body);

    // Verify we get taskCreated event
    assert!(
        text.contains("event: taskCreated"),
        "Should have taskCreated event, got: {text}"
    );

    // Verify the data contains the task in working state
    assert!(
        text.contains("working") || text.contains("\"state\":\"working\""),
        "Task should be in working state"
    );

    // Verify we also get a cancel status update
    assert!(
        text.contains("event: taskStatusUpdate"),
        "Should have taskStatusUpdate event for cancel, got: {text}"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_send_stream_invalid_body() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/tasks/sendStream", server.local_url))
        .json(&json!({"id": "req-1"})) // missing message
        .send()
        .await
        .unwrap();

    // Read the response body and check for error
    let body = resp.bytes().await.unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains("-32602") || text.contains("Invalid params"),
        "Should return error for invalid body, got: {text}"
    );

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// Incoming task event channel tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_incoming_task_channel() {
    ensure_crypto_provider();
    let card = AgentCard {
        name: "test".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let mut server = A2aServer::start(card, Arc::new(PeerCache::new()))
        .await
        .unwrap();
    let mut task_rx = server.take_incoming_task_receiver().unwrap();

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
    };
    let client = test_client();

    // Send a task with senderUrl
    let resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&serde_json::json!({
            "id": "req-1",
            "message": serde_json::to_value(&msg).unwrap(),
            "sessionId": "sess-1",
            "senderUrl": "http://sender.local:12345"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Verify the event was received
    let incoming = task_rx.try_recv().unwrap();
    assert_eq!(incoming.task_id.len(), 36, "should be UUID");
    assert_eq!(incoming.sender_url, "http://sender.local:12345");
    assert_eq!(incoming.session_id, Some("sess-1".into()));

    // Verify message content
    if let Part::Text { text } = &incoming.message.parts[0] {
        assert_eq!(text, "hello");
    } else {
        panic!("expected text part");
    }

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// PeerCache population from sender_url
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sender_url_populates_peer_cache() {
    ensure_crypto_provider();

    let cache = Arc::new(PeerCache::new());
    let card = AgentCard {
        name: "test".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start(card, cache.clone()).await.unwrap();

    let client = test_client();

    // Send a task with senderUrl
    let resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({
            "id": "req-1",
            "message": {
                "role": "user",
                "parts": [{"type": "text", "text": "hello"}]
            },
            "senderUrl": "http://sender.local:12345"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Verify peer cache has the sender
    let peers = cache.list();
    assert_eq!(peers.len(), 1, "PeerCache should have 1 entry");
    assert_eq!(peers[0].url, "http://sender.local:12345");
    assert_eq!(peers[0].host, "sender.local");
    assert_eq!(peers[0].port, 12345);

    server.shutdown().await;
}

#[tokio::test]
async fn test_sender_url_empty_does_not_populate_cache() {
    ensure_crypto_provider();

    let cache = Arc::new(PeerCache::new());
    let card = AgentCard {
        name: "test".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start(card, cache.clone()).await.unwrap();

    let client = test_client();

    // Send a task without senderUrl
    let resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({
            "id": "req-1",
            "message": {
                "role": "user",
                "parts": [{"type": "text", "text": "hello"}]
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Verify peer cache is empty
    let peers = cache.list();
    assert_eq!(
        peers.len(),
        0,
        "PeerCache should be empty when no senderUrl"
    );

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// tasks.list endpoint
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_list_tasks_endpoint() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task first via send
    client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({"id": "r1", "message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();

    // List tasks
    let resp = client
        .post(format!("{}/tasks/list", server.local_url))
        .json(&json!({"id": "r2"}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["result"]["tasks"].as_array().unwrap().len(),
        1,
        "Should have 1 task"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_list_tasks_endpoint_with_filter() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task — it starts in Submitted, then transitions to Working
    let send_resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({"id": "r1", "message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["result"]["id"].as_str().unwrap().to_string();

    // Cancel it so it's in 'canceled' state
    client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    // Create another task — this one stays in 'working'
    client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({"id": "r2", "message": {"role": "user", "parts": [{"type":"text","text":"hello again"}]}}))
        .send()
        .await
        .unwrap();

    // List with filter: working
    let resp = client
        .post(format!("{}/tasks/list", server.local_url))
        .json(&json!({"id": "r3", "status": "working"}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let working_tasks = body["result"]["tasks"].as_array().unwrap();
    assert_eq!(working_tasks.len(), 1, "Should have 1 working task");
    assert_eq!(working_tasks[0]["status"]["state"], "working");

    // List with filter: canceled
    let resp = client
        .post(format!("{}/tasks/list", server.local_url))
        .json(&json!({"id": "r4", "status": "canceled"}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let canceled_tasks = body["result"]["tasks"].as_array().unwrap();
    assert_eq!(canceled_tasks.len(), 1, "Should have 1 canceled task");
    assert_eq!(canceled_tasks[0]["status"]["state"], "canceled");

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// Subscribe (SSE) endpoint
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_subscribe_stream_receives_events() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task
    let resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({"id": "r1", "message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["result"]["id"].as_str().unwrap().to_string();

    // Subscribe (opens SSE stream)
    let sse_resp = client
        .get(format!("{}/tasks/{}/subscribe", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    assert!(
        sse_resp.status().is_success(),
        "Subscribe should return 200"
    );

    // Verify SSE content type
    let content_type = sse_resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/event-stream"),
        "Expected SSE content type, got: {content_type}"
    );

    // Cancel the task — should trigger a status update SSE event
    client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .json(&json!({"id": "r2"}))
        .send()
        .await
        .unwrap();

    // Read SSE body — the stream should close after the cancel event
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let body_bytes = sse_resp.bytes().await.unwrap();
    let text = String::from_utf8_lossy(&body_bytes);

    assert!(
        text.contains("event: taskStatusUpdate"),
        "Should have taskStatusUpdate event, got: {text}"
    );
    assert!(
        text.contains("canceled"),
        "Status should be canceled, got: {text}"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_subscribe_task_not_found() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .get(format!("{}/tasks/nonexistent/subscribe", server.local_url))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("error").is_some(),
        "Should return error for nonexistent task"
    );
    assert_eq!(body["error"]["code"], -32001);

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// Push notification config endpoints
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_push_config_crud_endpoints() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task first
    let resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({"id": "r1", "message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["result"]["id"].as_str().unwrap().to_string();

    // Create a push config
    let create_resp = client
        .post(format!(
            "{}/tasks/{}/push-notifications",
            server.local_url, task_id
        ))
        .json(&json!({"id": "r2", "url": "https://hook.example.com/notify"}))
        .send()
        .await
        .unwrap();
    let create_body: serde_json::Value = create_resp.json().await.unwrap();
    let config_id = create_body["result"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        create_body["result"]["url"],
        "https://hook.example.com/notify"
    );

    // List push configs
    let list_resp = client
        .get(format!(
            "{}/tasks/{}/push-notifications",
            server.local_url, task_id
        ))
        .send()
        .await
        .unwrap();
    let list_body: serde_json::Value = list_resp.json().await.unwrap();
    let configs = list_body["result"]["configs"].as_array().unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0]["id"], config_id);

    // Get specific push config
    let get_resp = client
        .get(format!(
            "{}/tasks/{}/push-notifications/{}",
            server.local_url, task_id, config_id
        ))
        .send()
        .await
        .unwrap();
    let get_body: serde_json::Value = get_resp.json().await.unwrap();
    assert_eq!(get_body["result"]["id"], config_id);

    // Delete push config
    let del_resp = client
        .delete(format!(
            "{}/tasks/{}/push-notifications/{}",
            server.local_url, task_id, config_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(del_resp.status(), 200);

    // Verify deleted
    let list2_resp = client
        .get(format!(
            "{}/tasks/{}/push-notifications",
            server.local_url, task_id
        ))
        .send()
        .await
        .unwrap();
    let list2_body: serde_json::Value = list2_resp.json().await.unwrap();
    let configs2 = list2_body["result"]["configs"].as_array().unwrap();
    assert!(configs2.is_empty(), "Push config should be deleted");

    server.shutdown().await;
}

#[tokio::test]
async fn test_push_config_not_found() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task
    let resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({"id": "r1", "message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["result"]["id"].as_str().unwrap().to_string();

    // Get nonexistent config
    let resp = client
        .get(format!(
            "{}/tasks/{}/push-notifications/nonexistent",
            server.local_url, task_id
        ))
        .send()
        .await
        .unwrap();
    let resp_body: serde_json::Value = resp.json().await.unwrap();
    assert!(resp_body.get("error").is_some());
    assert_eq!(resp_body["error"]["code"], -32001);

    server.shutdown().await;
}

#[tokio::test]
async fn test_push_config_missing_url() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task
    let resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({"id": "r1", "message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["result"]["id"].as_str().unwrap().to_string();

    // Attempt to create push config without URL
    let resp = client
        .post(format!(
            "{}/tasks/{}/push-notifications",
            server.local_url, task_id
        ))
        .json(&json!({"id": "r2"}))
        .send()
        .await
        .unwrap();
    let resp_body: serde_json::Value = resp.json().await.unwrap();
    assert!(resp_body.get("error").is_some());
    assert_eq!(resp_body["error"]["code"], -32602);

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// File exchange (A2A spec §6.7)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_file_upload_and_download_roundtrip() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Upload a file
    let file_content = b"hello, a2a file exchange!";
    let upload_resp = client
        .post(format!("{}/files/upload", server.local_url))
        .body(file_content.to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(upload_resp.status(), 200);

    let upload_body: serde_json::Value = upload_resp.json().await.unwrap();
    let file_id = upload_body["id"].as_str().unwrap().to_string();
    assert!(!file_id.is_empty(), "file ID should not be empty");

    let file_url = upload_body["url"].as_str().unwrap().to_string();
    assert!(
        file_url.contains(&file_id),
        "URL should contain the file ID"
    );

    // Download the file
    let download_resp = client
        .get(format!("{}/files/{}", server.local_url, file_id))
        .send()
        .await
        .unwrap();
    assert_eq!(download_resp.status(), 200);

    let downloaded = download_resp.bytes().await.unwrap();
    assert_eq!(
        downloaded.as_ref(),
        file_content,
        "downloaded content should match uploaded content"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_file_download_not_found() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .get(format!("{}/files/nonexistent-id", server.local_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "non-existent file should return 404");

    server.shutdown().await;
}

#[tokio::test]
async fn test_file_upload_multiple_files() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Upload two files
    let resp1 = client
        .post(format!("{}/files/upload", server.local_url))
        .body(b"file one content".to_vec())
        .send()
        .await
        .unwrap();
    let body1: serde_json::Value = resp1.json().await.unwrap();
    let id1 = body1["id"].as_str().unwrap().to_string();

    let resp2 = client
        .post(format!("{}/files/upload", server.local_url))
        .body(b"file two content".to_vec())
        .send()
        .await
        .unwrap();
    let body2: serde_json::Value = resp2.json().await.unwrap();
    let id2 = body2["id"].as_str().unwrap().to_string();

    assert_ne!(id1, id2, "each upload should get a unique ID");

    // Download and verify both
    let dl1 = client
        .get(format!("{}/files/{}", server.local_url, id1))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(dl1.as_ref(), b"file one content");

    let dl2 = client
        .get(format!("{}/files/{}", server.local_url, id2))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(dl2.as_ref(), b"file two content");

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_send_with_idempotency_key() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // First request with idempotencyKey
    let resp1 = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({
            "id": "req-1",
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
            "idempotencyKey": "idem-1"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp1.status(), 200);
    let body1: serde_json::Value = resp1.json().await.unwrap();
    let task_id1 = body1["result"]["id"].as_str().unwrap().to_string();

    // Second request with same idempotencyKey should return same task
    let resp2 = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({
            "id": "req-2",
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello again"}]},
            "idempotencyKey": "idem-1"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), 200);
    let body2: serde_json::Value = resp2.json().await.unwrap();
    let task_id2 = body2["result"]["id"].as_str().unwrap().to_string();

    assert_eq!(
        task_id1, task_id2,
        "same idempotencyKey should return the same task"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_send_with_different_idempotency_keys() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp1 = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({
            "id": "req-1",
            "message": {"role": "user", "parts": [{"type": "text", "text": "first"}]},
            "idempotencyKey": "key-a"
        }))
        .send()
        .await
        .unwrap();
    let body1: serde_json::Value = resp1.json().await.unwrap();
    let task_id1 = body1["result"]["id"].as_str().unwrap().to_string();

    let resp2 = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({
            "id": "req-2",
            "message": {"role": "user", "parts": [{"type": "text", "text": "second"}]},
            "idempotencyKey": "key-b"
        }))
        .send()
        .await
        .unwrap();
    let body2: serde_json::Value = resp2.json().await.unwrap();
    let task_id2 = body2["result"]["id"].as_str().unwrap().to_string();

    assert_ne!(
        task_id1, task_id2,
        "different idempotencyKeys should create different tasks"
    );

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// Multi-turn support: contextId on tasks.send
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_tasks_send_with_context_id() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({
            "id": "req-1",
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
            "contextId": "ctx-123"
        }))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["result"]["contextId"], "ctx-123",
        "contextId should be stored"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_send_with_parent_task_id() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({
            "id": "req-1",
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
            "parentTaskId": "parent-456"
        }))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["result"]["parentTaskId"], "parent-456",
        "parentTaskId should be stored"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_send_with_context_and_parent() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({
            "id": "req-1",
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
            "contextId": "ctx-123",
            "parentTaskId": "parent-456",
            "sessionId": "sess-789"
        }))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["result"]["contextId"], "ctx-123",
        "contextId should be stored"
    );
    assert_eq!(
        body["result"]["parentTaskId"], "parent-456",
        "parentTaskId should be stored"
    );
    assert_eq!(
        body["result"]["sessionId"], "sess-789",
        "sessionId should be stored"
    );

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// Cache-Control and ETag headers on agent.json (§8.6)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_agent_card_cache_headers() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .get(format!("{}/agent.json", server.local_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let cache_control = resp
        .headers()
        .get("Cache-Control")
        .and_then(|v| v.to_str().ok());
    assert_eq!(
        cache_control,
        Some("max-age=300"),
        "agent.json should have Cache-Control: max-age=300"
    );

    let etag = resp.headers().get("ETag").and_then(|v| v.to_str().ok());
    assert!(etag.is_some(), "agent.json should have an ETag header");
    assert!(etag.unwrap().starts_with('"'), "ETag should be quoted");
    // With the test server card, version is "1.0"
    assert_eq!(etag, Some(r#""1.0""#), "ETag should match card version");

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// Extended agent card endpoint (§9.4.8)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_extended_agent_card() {
    ensure_crypto_provider();

    let card = AgentCard {
        name: "test".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start(card, Arc::new(PeerCache::new()))
        .await
        .unwrap();

    let resp = reqwest::get(format!("{}/agent.json/extended", server.local_url))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("agentCard").is_some(), "Should have agentCard");
    assert!(
        body.get("extendedCapabilities").is_some(),
        "Should have extendedCapabilities"
    );
    assert_eq!(body["extendedCapabilities"]["streaming"], true);
    assert_eq!(body["extendedCapabilities"]["pushNotifications"], false);
    assert_eq!(body["extendedCapabilities"]["subscribeToTask"], true);
    assert_eq!(body["extendedCapabilities"]["listTasks"], true);
    assert_eq!(
        body["provider"]["organization"], "nu-agent",
        "provider.organization should be nu-agent"
    );

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// Content type validation on tasks.send
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_tasks_send_unsupported_content_type() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({
            "id": "req-1",
            "message": {
                "role": "user",
                "parts": [{"type": "unknown_type", "data": "something"}]
            }
        }))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("error").is_some(),
        "Should return error for unsupported content type"
    );
    assert_eq!(
        body["error"]["code"], -32002,
        "Should use CONTENT_TYPE_NOT_SUPPORTED code"
    );
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("unknown_type"),
        "Error message should mention unsupported type"
    );

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// Task metadata roundtrip (§4.1.1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_tasks_send_with_metadata() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({
            "id": "req-1",
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
            "metadata": {"source": "test", "priority": 5}
        }))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["result"]["metadata"]["source"], "test",
        "metadata.source should be stored"
    );
    assert_eq!(
        body["result"]["metadata"]["priority"], 5,
        "metadata.priority should be stored"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_get_returns_metadata() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task with metadata
    let send_resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({
            "id": "req-1",
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
            "metadata": {"source": "test", "key": "value"}
        }))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["result"]["id"].as_str().unwrap().to_string();

    // Get the task and verify metadata is present
    let resp = client
        .get(format!("{}/tasks/{task_id}", server.local_url))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["result"]["metadata"]["source"], "test",
        "metadata should be preserved when getting task"
    );
    assert_eq!(
        body["result"]["metadata"]["key"], "value",
        "metadata should be preserved when getting task"
    );

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// historyLength on tasks.get
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_task_with_history_length_full() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task via send (adds message to history)
    let send_resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({"id": "req-1", "message": {"role": "user", "parts": [{"type":"text","text":"turn 1"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["result"]["id"].as_str().unwrap().to_string();

    // Get the task with no historyLength (should return full history)
    let resp = client
        .get(format!("{}/tasks/{}", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let history = body["result"]["history"].as_array();
    assert!(history.is_some(), "history should be present");
    assert_eq!(history.unwrap().len(), 1, "should have 1 history entry");

    server.shutdown().await;
}

#[tokio::test]
async fn test_get_task_with_history_length_filter() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;
    let store = server.task_store();

    // Create a task via send (adds first message to history)
    let send_resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({"id": "req-1", "message": {"role": "user", "parts": [{"type":"text","text":"turn 1"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["result"]["id"].as_str().unwrap().to_string();

    // Append more messages to history directly (simulate multi-turn)
    store
        .append_history(
            &task_id,
            Message {
                role: Role::Agent,
                parts: vec![Part::Text {
                    text: "response 1".into(),
                }],
            },
        )
        .unwrap();
    store
        .append_history(
            &task_id,
            Message {
                role: Role::User,
                parts: vec![Part::Text {
                    text: "turn 2".into(),
                }],
            },
        )
        .unwrap();

    // Get the task with historyLength=1 (should return only last entry)
    let resp = client
        .get(format!(
            "{}/tasks/{}?historyLength=1",
            server.local_url, task_id
        ))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let history = body["result"]["history"].as_array().unwrap();
    assert_eq!(history.len(), 1, "historyLength=1 should return 1 entry");
    assert_eq!(
        history[0]["role"], "user",
        "last entry should be the user turn"
    );
    assert_eq!(history[0]["parts"][0]["text"], "turn 2");

    server.shutdown().await;
}

#[tokio::test]
async fn test_get_task_with_history_length_zero() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task
    let send_resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({"id": "req-1", "message": {"role": "user", "parts": [{"type":"text","text":"hello"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["result"]["id"].as_str().unwrap().to_string();

    // Get the task with historyLength=0 (should omit history)
    let resp = client
        .get(format!(
            "{}/tasks/{}?historyLength=0",
            server.local_url, task_id
        ))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["result"].get("history").is_none(),
        "history should be absent when historyLength=0"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_get_task_with_history_length_invalid() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;
    let store = server.task_store();

    // Create a task
    let send_resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({"id": "req-1", "message": {"role": "user", "parts": [{"type":"text","text":"hello"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["result"]["id"].as_str().unwrap().to_string();

    // Append some history
    store
        .append_history(
            &task_id,
            Message {
                role: Role::Agent,
                parts: vec![Part::Text {
                    text: "response".into(),
                }],
            },
        )
        .unwrap();

    // Get with invalid historyLength (non-numeric) — should return full history
    let resp = client
        .get(format!(
            "{}/tasks/{}?historyLength=invalid",
            server.local_url, task_id
        ))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let history = body["result"]["history"].as_array();
    assert!(
        history.is_some(),
        "history should be present when historyLength is invalid"
    );
    assert_eq!(history.unwrap().len(), 2, "full history should be returned");

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// IncomingTask channel with contextId and parentTaskId
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_incoming_task_channel_with_context_and_parent() {
    ensure_crypto_provider();
    let card = AgentCard {
        name: "test".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let mut server = A2aServer::start(card, Arc::new(PeerCache::new()))
        .await
        .unwrap();
    let mut task_rx = server.take_incoming_task_receiver().unwrap();

    let client = test_client();

    // Send a task with contextId and parentTaskId
    let resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({
            "id": "req-1",
            "message": {"role": "user", "parts": [{"type": "text", "text": "multi-turn"}]},
            "contextId": "ctx-999",
            "parentTaskId": "parent-888",
            "sessionId": "sess-777"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Verify the event was received with all fields
    let incoming = task_rx.try_recv().unwrap();
    assert_eq!(incoming.context_id, Some("ctx-999".into()));
    assert_eq!(incoming.parent_task_id, Some("parent-888".into()));
    assert_eq!(incoming.session_id, Some("sess-777".into()));

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// A2A-Version header rejection tests (§9.2, §14.2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_a2a_version_missing_rejected() {
    ensure_crypto_provider();
    let card = AgentCard {
        name: "test".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start(card, Arc::new(PeerCache::new()))
        .await
        .unwrap();
    let client = reqwest::Client::new(); // no A2A-Version header

    let resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({
            "id": "r1",
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]}
        }))
        .send()
        .await
        .unwrap();
    // JSON-RPC errors are returned with HTTP 200
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], crate::UNSUPPORTED_OPERATION);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("A2A-Version")
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_a2a_version_unsupported_value_rejected() {
    ensure_crypto_provider();
    let card = AgentCard {
        name: "test".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start(card, Arc::new(PeerCache::new()))
        .await
        .unwrap();
    let client = reqwest::Client::builder()
        .default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::HeaderName::from_static("a2a-version"),
                reqwest::header::HeaderValue::from_static("0.9"),
            );
            headers
        })
        .build()
        .unwrap();

    let resp = client
        .post(format!("{}/tasks/send", server.local_url))
        .json(&json!({
            "id": "r1",
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], crate::UNSUPPORTED_OPERATION);

    server.shutdown().await;
}

#[tokio::test]
async fn test_agent_json_bypasses_version_check() {
    ensure_crypto_provider();
    let card = AgentCard {
        name: "test".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start(card, Arc::new(PeerCache::new()))
        .await
        .unwrap();
    let client = reqwest::Client::new(); // no A2A-Version header

    // agent.json should work without A2A-Version
    let resp = client
        .get(format!("{}/agent.json", server.local_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "test");

    server.shutdown().await;
}

#[tokio::test]
async fn test_extended_agent_card_bypasses_version_check() {
    ensure_crypto_provider();
    let card = AgentCard {
        name: "test".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start(card, Arc::new(PeerCache::new()))
        .await
        .unwrap();
    let client = reqwest::Client::new(); // no A2A-Version header

    // /agent.json/extended should work without A2A-Version
    let resp = client
        .get(format!("{}/agent.json/extended", server.local_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("agentCard").is_some());

    server.shutdown().await;
}
