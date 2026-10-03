use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use crate::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

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

/// Parse the `data:` payloads of an SSE body into JSON values.
fn sse_data_events(text: &str) -> Vec<serde_json::Value> {
    text.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str(data).ok())
        .collect()
}

/// Assert the error metadata `timestamp` uses the spec §5.6.1 format
/// `YYYY-MM-DDTHH:mm:ss.sssZ` — a `Z` suffix, never a `+00:00` offset.
fn assert_timestamp_has_z_suffix(body: &serde_json::Value) {
    let ts = body["error"]["details"][0]["metadata"]["timestamp"]
        .as_str()
        .unwrap_or("");
    assert!(ts.ends_with('Z'), "timestamp must end with 'Z', got: {ts}");
    assert!(
        !ts.contains('+'),
        "timestamp must not contain a timezone offset, got: {ts}"
    );
    assert!(
        ts.len() == 24 && ts.as_bytes()[10] == b'T' && ts.as_bytes()[19] == b'.',
        "timestamp must match YYYY-MM-DDTHH:mm:ss.sssZ, got: {ts}"
    );
}

/// Assert every timestamp field on a serialized task uses the spec §5.6.1
/// format `YYYY-MM-DDTHH:mm:ss.sssZ` — a `Z` suffix, never a `+00:00` offset.
fn assert_task_timestamps_have_z_suffix(task: &serde_json::Value) {
    let ts = task["status"]["timestamp"].as_str().unwrap_or("");
    assert!(
        ts.ends_with('Z'),
        "status.timestamp must end with 'Z', got: {ts}"
    );
    assert!(
        !ts.contains('+'),
        "status.timestamp must not contain a timezone offset, got: {ts}"
    );
    if let Some(created_at) = task.get("created_at").and_then(|v| v.as_str()) {
        assert!(
            created_at.ends_with('Z'),
            "created_at must end with 'Z', got: {created_at}"
        );
        assert!(
            !created_at.contains('+'),
            "created_at must not contain a timezone offset, got: {created_at}"
        );
    }
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
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
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
        .get(format!("{}/tasks", server.local_url))
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

    // /.well-known/agent-card.json endpoint
    let resp = client
        .get(format!("{}/.well-known/agent-card.json", server.local_url))
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
        "agent card response should include A2A-Version header"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_server_cleanup_frees_port() -> Result<()> {
    ensure_crypto_provider();

    let card = AgentCard {
        name: "cleanup-test".to_string(),
        url: "http://127.0.0.1:0".to_string(),
        ..Default::default()
    };
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();
    let port = server.port;

    server.shutdown().await;

    // Give the OS time to release the TCP port from TIME_WAIT state.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Should be able to bind to same port now
    tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .map_err(|e| format!("port should be free after shutdown: {e:?}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Route handler integration tests (against real running server)
// ---------------------------------------------------------------------------

async fn test_server() -> (A2aServer, reqwest::Client) {
    let card = AgentCard {
        name: "test-agent".into(),
        url: "http://127.0.0.1:0".into(),
        version: "1.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: "http://127.0.0.1:0".into(),
            protocol_version: "1.0".into(),
            protocol_binding: PROTOCOL_BINDING.into(),
        }],
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
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();
    let client = test_client();
    (server, client)
}

#[tokio::test]
async fn test_agent_card_endpoint() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .get(format!("{}/.well-known/agent-card.json", server.local_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "test-agent");
    let skills = body["skills"]
        .as_array()
        .ok_or("should have skills array")?;
    assert_eq!(skills.len(), 1);
    assert_eq!(body["skills"][0]["name"], "Test");

    // protocolBinding documents the colon-action sub-path deviation
    let binding = body["supportedInterfaces"][0]["protocolBinding"]
        .as_str()
        .ok_or("should have protocolBinding")?;
    assert!(
        binding.contains("subpath-actions"),
        "protocolBinding should document the sub-path deviation, got: {binding}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_tasks_send_creates_task() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
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
    assert!(body.get("task").is_some(), "Should have a task field");
    let task_id = body["task"]["id"].as_str().ok_or("should have task id")?;
    assert!(!task_id.is_empty(), "Task should have an ID");
    assert_eq!(body["task"]["status"]["state"], "TASK_STATE_WORKING");

    server.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// SendMessageConfiguration / blocking semantics (spec §3.2.2)
// ---------------------------------------------------------------------------

/// Start a server whose blocking deadline is short enough for tests.
async fn test_server_with_blocking_timeout(timeout: Duration) -> (A2aServer, reqwest::Client) {
    let card = AgentCard {
        name: "test-agent".into(),
        url: "http://127.0.0.1:0".into(),
        version: "1.0".into(),
        ..Default::default()
    };
    let server =
        A2aServer::start_with_blocking_timeout(card, Arc::new(PeerCache::default()), 0, timeout)
            .await
            .unwrap();
    let client = test_client();
    (server, client)
}

#[tokio::test]
async fn test_send_with_return_immediately_returns_working_task() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
            "configuration": {"returnImmediately": true}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["task"]["status"]["state"], "TASK_STATE_WORKING",
        "non-blocking send must return the in-progress task, got: {body}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_without_configuration_blocks_until_terminal() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server_with_blocking_timeout(MAX_TEST_BLOCKING_TIMEOUT).await;

    // Cancel the task from another connection while the send request blocks.
    let store = server.task_store();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        if let Some(task) = store.list_tasks(None).first() {
            let _ = store.update_status(&task.id, TaskState::Canceled, None);
        }
    });

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "block me"}]}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["task"]["status"]["state"], "TASK_STATE_CANCELED",
        "blocking send must return the terminal task, got: {body}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_blocking_timeout_returns_non_terminal_task() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server_with_blocking_timeout(Duration::from_millis(200)).await;

    let started = std::time::Instant::now();
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "never finishes"}]},
            "configuration": {"returnImmediately": false}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(
        started.elapsed() >= Duration::from_millis(200),
        "blocking send must wait for the deadline, elapsed: {:?}",
        started.elapsed()
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["task"]["status"]["state"], "TASK_STATE_WORKING",
        "timed-out blocking send must return the non-terminal task, got: {body}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_with_accepted_output_modes_parses_without_error() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
            "configuration": {
                "returnImmediately": true,
                "acceptedOutputModes": ["text/plain", "application/json"]
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("task").is_some(),
        "acceptedOutputModes must parse without error, got: {body}"
    );

    server.shutdown().await;
    Ok(())
}

/// Start a server whose card advertises the given `defaultOutputModes`.
async fn test_server_with_output_modes(modes: Vec<String>) -> (A2aServer, reqwest::Client) {
    let card = AgentCard {
        name: "test-agent".into(),
        url: "http://127.0.0.1:0".into(),
        version: "1.0".into(),
        default_output_modes: modes,
        ..Default::default()
    };
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();
    let client = test_client();
    (server, client)
}

#[tokio::test]
async fn test_send_without_accepted_output_modes_succeeds() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
            "configuration": {"returnImmediately": true}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("task").is_some(),
        "absent acceptedOutputModes must not be rejected, got: {body}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_rejects_unsupported_accepted_output_modes() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
            "configuration": {
                "returnImmediately": true,
                "acceptedOutputModes": ["application/json"]
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        400,
        "acceptedOutputModes with no intersection must be rejected"
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(
        body["error"]["details"][0]["reason"],
        "CONTENT_TYPE_NOT_SUPPORTED"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_accepts_card_advertised_output_mode() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) =
        test_server_with_output_modes(vec!["application/json".to_string()]).await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
            "configuration": {
                "returnImmediately": true,
                "acceptedOutputModes": ["application/json"]
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "a mode advertised by the card must be accepted"
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("task").is_some(),
        "should create a task, got: {body}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_rejects_mode_not_advertised_by_card() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) =
        test_server_with_output_modes(vec!["application/json".to_string()]).await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
            "configuration": {
                "returnImmediately": true,
                "acceptedOutputModes": ["text/plain"]
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        400,
        "a mode the card does not advertise must be rejected"
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["error"]["details"][0]["reason"],
        "CONTENT_TYPE_NOT_SUPPORTED"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_stream_without_accepted_output_modes_succeeds() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "stream"}]},
            "configuration": {"returnImmediately": true}
        }))
        .send()
        .await
        .unwrap();

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/event-stream"),
        "absent acceptedOutputModes must not be rejected, got: {content_type}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_stream_accepts_matching_accepted_output_modes() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "stream"}]},
            "configuration": {
                "returnImmediately": true,
                "acceptedOutputModes": ["text/plain"]
            }
        }))
        .send()
        .await
        .unwrap();

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/event-stream"),
        "matching acceptedOutputModes must be accepted, got: {content_type}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_stream_rejects_unsupported_accepted_output_modes() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "stream"}]},
            "configuration": {
                "returnImmediately": true,
                "acceptedOutputModes": ["application/json"]
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        400,
        "acceptedOutputModes with no intersection must be rejected"
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(
        body["error"]["details"][0]["reason"],
        "CONTENT_TYPE_NOT_SUPPORTED"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_stream_accepts_configuration() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "stream"}]},
            "configuration": {"returnImmediately": true}
        }))
        .send()
        .await
        .unwrap();

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/event-stream"),
        "streaming must ignore returnImmediately and stay a stream, got: {content_type}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_malformed_configuration_returns_400() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
            "configuration": {"returnImmediately": "yes"}
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        400,
        "malformed configuration must be rejected, not silently defaulted"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(body["error"]["status"], "INVALID_ARGUMENT");
    assert_eq!(body["error"]["details"][0]["reason"], "INVALID_ARGUMENT");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_stream_malformed_configuration_returns_400() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "stream"}]},
            "configuration": {"returnImmediately": "yes"}
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        400,
        "malformed configuration must be rejected, not silently ignored"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(body["error"]["status"], "INVALID_ARGUMENT");
    assert_eq!(body["error"]["details"][0]["reason"], "INVALID_ARGUMENT");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_tasks_send_missing_message() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("error").is_some(),
        "Should return error for missing message"
    );
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(body["error"]["status"], "BAD_REQUEST");
    assert_eq!(body["error"]["details"][0]["reason"], "INVALID_ARGUMENT");
    assert_eq!(body["error"]["details"][0]["domain"], "a2a-protocol.org");

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_get_returns_task() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task first
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Get the task
    let resp = client
        .get(format!("{}/tasks/{}", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["task"]["id"], task_id);
    assert_eq!(body["task"]["status"]["state"], "TASK_STATE_WORKING");
    assert_task_timestamps_have_z_suffix(&body["task"]);

    server.shutdown().await;
    Ok(())
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
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("error").is_some());
    assert_eq!(body["error"]["code"], 404);
    assert_eq!(body["error"]["status"], "NOT_FOUND");
    assert_eq!(body["error"]["details"][0]["reason"], "TASK_NOT_FOUND");
    assert_eq!(body["error"]["details"][0]["domain"], "a2a-protocol.org");
    assert_timestamp_has_z_suffix(&body);

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_cancel_not_found() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/tasks/nonexistent-id/cancel", server.local_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("error").is_some());
    assert_eq!(body["error"]["code"], 404);
    assert_eq!(body["error"]["status"], "NOT_FOUND");
    assert_eq!(body["error"]["details"][0]["reason"], "TASK_NOT_FOUND");
    assert_eq!(body["error"]["details"][0]["domain"], "a2a-protocol.org");
    assert_timestamp_has_z_suffix(&body);

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_cancel() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create task
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(
            &json!({"message": {"role": "user", "parts": [{"type": "text", "text": "cancel me"}]}}),
        )
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Cancel it
    let resp = client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["task"]["status"]["state"], "TASK_STATE_CANCELED");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_cancel_completed_fails() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type": "text", "text": "done"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

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
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("error").is_some(), "Second cancel should fail");
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(body["error"]["status"], "INVALID_REQUEST");
    assert_eq!(body["error"]["details"][0]["reason"], "TASK_NOT_CANCELABLE");
    assert_eq!(body["error"]["details"][0]["domain"], "a2a-protocol.org");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_task_lifecycle_full() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();
    assert_eq!(send_body["task"]["status"]["state"], "TASK_STATE_WORKING");

    // Get
    let get_resp = client
        .get(format!("{}/tasks/{}", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    let get_body: serde_json::Value = get_resp.json().await.unwrap();
    assert_eq!(get_body["task"]["status"]["state"], "TASK_STATE_WORKING");

    // Cancel
    let cancel_resp = client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    let cancel_body: serde_json::Value = cancel_resp.json().await.unwrap();
    assert_eq!(
        cancel_body["task"]["status"]["state"],
        "TASK_STATE_CANCELED"
    );

    // Get after cancel
    let get2_resp = client
        .get(format!("{}/tasks/{}", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    let get2_body: serde_json::Value = get2_resp.json().await.unwrap();
    assert_eq!(get2_body["task"]["status"]["state"], "TASK_STATE_CANCELED");

    server.shutdown().await;
    Ok(())
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
            c.post(format!("{u}/message:send"))
                .json(&json!({"message": {"role": "user", "parts": [{"type": "text", "text": format!("msg-{i}")}]}}))
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
async fn test_send_stream_returns_sse() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "stream"}]},
            "contextId": "ctx-abc"
        }))
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
        .get(format!("{}/tasks", server.local_url))
        .send()
        .await
        .unwrap();
    let list_body: serde_json::Value = list_resp.json().await.unwrap();
    let tasks = list_body["tasks"]
        .as_array()
        .ok_or("should have tasks array")?;
    let task_id = tasks
        .iter()
        .find(|t| t["status"]["state"] == "TASK_STATE_WORKING")
        .and_then(|t| t["id"].as_str())
        .ok_or("should find a working task")?;

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

    // Verify we get a task event (StreamResponse format)
    assert!(
        text.contains(r#""task""#),
        "Should have task event with StreamResponse, got: {text}"
    );

    // Verify the data contains the task in working state
    assert!(
        text.contains("TASK_STATE_WORKING"),
        "Task should be in working state, got: {text}"
    );

    // Verify we also get a cancel status update (statusUpdate format)
    assert!(
        text.contains("statusUpdate"),
        "Should have statusUpdate event for cancel, got: {text}"
    );

    // Spec §4.2.1: every statusUpdate event carries the task's contextId
    let events = sse_data_events(&text);
    let status_updates: Vec<&serde_json::Value> = events
        .iter()
        .filter_map(|e| e.get("statusUpdate"))
        .collect();
    assert!(
        !status_updates.is_empty(),
        "Should have at least one statusUpdate event, got: {text}"
    );
    for update in status_updates {
        assert_eq!(
            update["contextId"], "ctx-abc",
            "statusUpdate must carry the task contextId, got: {update}"
        );
    }

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_stream_invalid_body() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({})) // missing message
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);

    // Read the response body and check for error
    let body = resp.bytes().await.unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains("400") || text.contains("BAD_REQUEST"),
        "Should return error for invalid body, got: {text}"
    );

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// Incoming task event channel tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_incoming_task_channel() -> Result<()> {
    ensure_crypto_provider();
    let card = AgentCard {
        name: "test".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let mut server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();
    let mut task_rx = server
        .take_incoming_task_receiver()
        .ok_or("should have incoming task receiver")?;

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    let client = test_client();

    // Send a task with senderUrl
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&serde_json::json!({
            "message": serde_json::to_value(&msg).unwrap(),
            "senderUrl": "http://sender.local:12345"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Verify the event was received
    let incoming = task_rx.try_recv().map_err(|e| format!("{e:?}"))?;
    assert_eq!(incoming.task_id.len(), 36, "should be UUID");
    assert_eq!(incoming.sender_url, "http://sender.local:12345");

    // Verify message content
    if let Part::Text { text } = &incoming.message.parts[0] {
        assert_eq!(text, "hello");
    } else {
        panic!("expected text part");
    }

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_task_cancel_channel_emits_task_id() -> Result<()> {
    ensure_crypto_provider();
    let card = AgentCard {
        name: "test".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let mut server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();
    let mut cancel_rx = server
        .take_task_cancel_receiver()
        .ok_or("should have cancel receiver")?;
    let client = test_client();

    // Create a task
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(
            &json!({"message": {"role": "user", "parts": [{"type": "text", "text": "cancel me"}]}}),
        )
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Cancel the task
    let resp = client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["task"]["status"]["state"], "TASK_STATE_CANCELED");

    // Verify the cancel channel received the task ID
    let received = cancel_rx.try_recv().map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        received, task_id,
        "cancel channel should deliver the task ID"
    );

    server.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// PeerCache population from sender_url
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sender_url_populates_peer_cache() {
    ensure_crypto_provider();

    let cache = Arc::new(PeerCache::default());
    let card = AgentCard {
        name: "test".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server =
        A2aServer::start_with_blocking_timeout(card, cache.clone(), 0, TEST_BLOCKING_TIMEOUT)
            .await
            .unwrap();

    let client = test_client();

    // Send a task with senderUrl
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
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

    let cache = Arc::new(PeerCache::default());
    let card = AgentCard {
        name: "test".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server =
        A2aServer::start_with_blocking_timeout(card, cache.clone(), 0, TEST_BLOCKING_TIMEOUT)
            .await
            .unwrap();

    let client = test_client();

    // Send a task without senderUrl
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
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
async fn test_list_tasks_endpoint() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task first via send
    client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();

    // List tasks
    let resp = client
        .get(format!("{}/tasks", server.local_url))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let tasks = body["tasks"].as_array().ok_or("should have tasks array")?;
    assert_eq!(tasks.len(), 1, "Should have 1 task");
    assert!(body.get("totalSize").is_some(), "Should have totalSize");
    assert!(body.get("pageSize").is_some(), "Should have pageSize");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_list_tasks_endpoint_with_filter() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task — it starts in Submitted, then transitions to Working
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Cancel it so it's in 'canceled' state
    client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    // Create another task — this one stays in 'working'
    client
        .post(format!("{}/message:send", server.local_url))
        .json(
            &json!({"message": {"role": "user", "parts": [{"type":"text","text":"hello again"}]}}),
        )
        .send()
        .await
        .unwrap();

    // List with filter: working
    let resp = client
        .get(format!(
            "{}/tasks?status=TASK_STATE_WORKING",
            server.local_url
        ))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let working_tasks = body["tasks"].as_array().ok_or("should have tasks array")?;
    assert_eq!(working_tasks.len(), 1, "Should have 1 working task");
    assert_eq!(working_tasks[0]["status"]["state"], "TASK_STATE_WORKING");

    // List with filter: canceled
    let resp = client
        .get(format!(
            "{}/tasks?status=TASK_STATE_CANCELED",
            server.local_url
        ))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let canceled_tasks = body["tasks"].as_array().ok_or("should have tasks array")?;
    assert_eq!(canceled_tasks.len(), 1, "Should have 1 canceled task");
    assert_eq!(canceled_tasks[0]["status"]["state"], "TASK_STATE_CANCELED");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_list_tasks_endpoint_invalid_status_returns_400() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .get(format!("{}/tasks?status=INVALID_BOGUS", server.local_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(body["error"]["status"], "INVALID_ARGUMENT");
    let message = body["error"]["message"]
        .as_str()
        .ok_or("should have error message")?;
    assert!(
        message.contains("INVALID_BOGUS"),
        "error message should name the invalid status, got: {message}"
    );

    server.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Subscribe (SSE) endpoint
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_subscribe_stream_receives_events() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type":"text","text":"hi"}]},
            "contextId": "ctx-abc"
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Subscribe (opens SSE stream)
    let sse_resp = client
        .post(format!("{}/tasks/{}/subscribe", server.local_url, task_id))
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
        .send()
        .await
        .unwrap();

    // Read SSE body — the stream should close after the cancel event
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let body_bytes = sse_resp.bytes().await.unwrap();
    let text = String::from_utf8_lossy(&body_bytes);

    // StreamResponse format has statusUpdate wrapper
    assert!(
        text.contains("statusUpdate"),
        "Should have statusUpdate event, got: {text}"
    );
    assert!(
        text.contains("TASK_STATE_CANCELED"),
        "Status should be canceled, got: {text}"
    );

    // Spec §4.2.1: every statusUpdate event carries the task's contextId
    let events = sse_data_events(&text);
    let status_updates: Vec<&serde_json::Value> = events
        .iter()
        .filter_map(|e| e.get("statusUpdate"))
        .collect();
    assert!(
        !status_updates.is_empty(),
        "Should have at least one statusUpdate event, got: {text}"
    );
    for update in status_updates {
        assert_eq!(
            update["contextId"], "ctx-abc",
            "statusUpdate must carry the task contextId, got: {update}"
        );
    }

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_subscribe_stream_artifact_update_carries_context_id() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task with a contextId
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type":"text","text":"hi"}]},
            "contextId": "ctx-abc"
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Subscribe (opens SSE stream)
    let sse_resp = client
        .post(format!("{}/tasks/{}/subscribe", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    assert!(
        sse_resp.status().is_success(),
        "Subscribe should return 200"
    );

    // Add an artifact — should trigger an artifactUpdate SSE event
    server
        .task_store()
        .add_artifact(
            &task_id,
            Artifact {
                artifact_id: "art-1".to_string(),
                name: Some("result".to_string()),
                parts: vec![Part::Text {
                    text: "output".into(),
                }],
                metadata: None,
            },
        )
        .map_err(|e| format!("add_artifact should succeed: {e:?}"))?;

    // Cancel the task to close the SSE stream
    client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let body_bytes = sse_resp.bytes().await.unwrap();
    let text = String::from_utf8_lossy(&body_bytes);

    // Spec §4.2.2: every artifactUpdate event carries the task's contextId
    let events = sse_data_events(&text);
    let artifact_updates: Vec<&serde_json::Value> = events
        .iter()
        .filter_map(|e| e.get("artifactUpdate"))
        .collect();
    assert!(
        !artifact_updates.is_empty(),
        "Should have at least one artifactUpdate event, got: {text}"
    );
    for update in artifact_updates {
        assert_eq!(update["taskId"], task_id);
        assert_eq!(
            update["contextId"], "ctx-abc",
            "artifactUpdate must carry the task contextId, got: {update}"
        );
    }

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_subscribe_task_not_found() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/tasks/nonexistent/subscribe", server.local_url))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("error").is_some(),
        "Should return error for nonexistent task"
    );
    assert_eq!(body["error"]["code"], 404);
    assert_eq!(body["error"]["status"], "NOT_FOUND");
    assert_eq!(body["error"]["details"][0]["reason"], "TASK_NOT_FOUND");
    assert_timestamp_has_z_suffix(&body);

    server.shutdown().await;
}

#[tokio::test]
async fn test_subscribe_terminal_task_returns_400() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task, then cancel it so it reaches a terminal state
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    // Subscribe to the terminal task — spec §3.1.6 requires an error
    let sse_resp = client
        .post(format!("{}/tasks/{}/subscribe", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    assert_eq!(
        sse_resp.status(),
        400,
        "subscribe to a terminal task must return 400"
    );

    let err_body: serde_json::Value = sse_resp.json().await.unwrap();
    assert_eq!(err_body["error"]["code"], 400);
    assert_eq!(
        err_body["error"]["details"][0]["reason"], "UNSUPPORTED_OPERATION",
        "reason must be UNSUPPORTED_OPERATION, got: {err_body}"
    );

    server.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Push notification config endpoints
// ---------------------------------------------------------------------------

/// Receiver end of the local webhook test server's payload channel.
type WebhookRx = tokio::sync::mpsc::Receiver<serde_json::Value>;

/// Start a local webhook receiver that forwards each received JSON body to the
/// returned channel. Returns the receiver's base URL.
async fn start_webhook_receiver() -> Result<(String, WebhookRx)> {
    let (tx, rx) = tokio::sync::mpsc::channel::<serde_json::Value>(4);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let app = axum::Router::new().route(
        "/hook",
        axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(body).await;
                axum::Json(serde_json::json!({ "ok": true }))
            }
        }),
    );

    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Ok((format!("http://127.0.0.1:{}/hook", addr.port()), rx))
}

#[tokio::test]
async fn test_push_notification_payload_is_stream_response_format() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;
    let (hook_url, mut hook_rx) = start_webhook_receiver().await?;

    // Create a task with a contextId so the payload can carry it
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type":"text","text":"hi"}]},
            "contextId": "ctx-push"
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Register the webhook
    let push_resp = client
        .post(format!(
            "{}/tasks/{}/pushNotificationConfigs",
            server.local_url, task_id
        ))
        .json(&json!({"url": hook_url}))
        .send()
        .await
        .unwrap();
    assert_eq!(push_resp.status(), 200);

    // Trigger a status change
    client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    // The webhook must receive a StreamResponse-shaped payload
    let payload = tokio::time::timeout(Duration::from_secs(5), hook_rx.recv())
        .await
        .map_err(|e| format!("timeout waiting for push notification: {e:?}"))?
        .ok_or("webhook channel closed")?;

    let update = payload
        .get("statusUpdate")
        .ok_or_else(|| format!("payload must use StreamResponse format, got: {payload}"))?;
    assert_eq!(update["taskId"], task_id);
    assert_eq!(update["contextId"], "ctx-push");
    assert_eq!(update["status"]["state"], "TASK_STATE_CANCELED");
    assert!(
        payload.get("StatusChanged").is_none(),
        "internal TaskEvent tag must not be sent to webhooks, got: {payload}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_push_config_crud_endpoints() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task first
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Create a push config
    let create_resp = client
        .post(format!(
            "{}/tasks/{}/pushNotificationConfigs",
            server.local_url, task_id
        ))
        .json(&json!({"url": "https://hook.example.com/notify"}))
        .send()
        .await
        .unwrap();
    let create_body: serde_json::Value = create_resp.json().await.unwrap();
    let config_id = create_body["id"]
        .as_str()
        .ok_or("should have config id")?
        .to_string();
    assert_eq!(create_body["url"], "https://hook.example.com/notify");

    // Get the single push config
    let get_resp = client
        .get(format!(
            "{}/tasks/{}/pushNotificationConfigs/{}",
            server.local_url, task_id, config_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(get_resp.status(), 200);
    let get_body: serde_json::Value = get_resp.json().await.unwrap();
    assert_eq!(get_body["id"], config_id);
    assert_eq!(get_body["url"], "https://hook.example.com/notify");

    // List push configs
    let list_resp = client
        .get(format!(
            "{}/tasks/{}/pushNotificationConfigs",
            server.local_url, task_id
        ))
        .send()
        .await
        .unwrap();
    let list_body: serde_json::Value = list_resp.json().await.unwrap();
    let configs = list_body["configs"]
        .as_array()
        .ok_or("should have configs array")?;
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0]["id"], config_id);

    // Delete push config
    let del_resp = client
        .delete(format!(
            "{}/tasks/{}/pushNotificationConfigs/{}",
            server.local_url, task_id, config_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(del_resp.status(), 200);

    // Verify deleted
    let list2_resp = client
        .get(format!(
            "{}/tasks/{}/pushNotificationConfigs",
            server.local_url, task_id
        ))
        .send()
        .await
        .unwrap();
    let list2_body: serde_json::Value = list2_resp.json().await.unwrap();
    let configs2 = list2_body["configs"]
        .as_array()
        .ok_or("should have configs array")?;
    assert!(configs2.is_empty(), "Push config should be deleted");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_push_config_not_found() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Get a nonexistent config — spec §11.3 GET single config returns 404
    let resp = client
        .get(format!(
            "{}/tasks/{}/pushNotificationConfigs/nonexistent",
            server.local_url, task_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    // Delete is idempotent, always succeeds
    let resp = client
        .delete(format!(
            "{}/tasks/{}/pushNotificationConfigs/nonexistent",
            server.local_url, task_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_push_config_missing_url() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Attempt to create push config without URL
    let resp = client
        .post(format!(
            "{}/tasks/{}/pushNotificationConfigs",
            server.local_url, task_id
        ))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let resp_body: serde_json::Value = resp.json().await.unwrap();
    assert!(resp_body.get("error").is_some());
    assert_eq!(resp_body["error"]["code"], 400);
    assert_eq!(resp_body["error"]["status"], "BAD_REQUEST");
    assert_eq!(
        resp_body["error"]["details"][0]["reason"],
        "INVALID_ARGUMENT"
    );

    server.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// File exchange (A2A spec §6.7)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_file_upload_and_download_roundtrip() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Upload a file
    let file_content = b"hello, a2a file exchange!";
    let upload_resp = client
        .post(format!("{}/files:upload", server.local_url))
        .body(file_content.to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(upload_resp.status(), 200);

    let upload_body: serde_json::Value = upload_resp.json().await.unwrap();
    let file_id = upload_body["id"]
        .as_str()
        .ok_or("should have file id")?
        .to_string();
    assert!(!file_id.is_empty(), "file ID should not be empty");

    let file_url = upload_body["url"]
        .as_str()
        .ok_or("should have file url")?
        .to_string();
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
    Ok(())
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
async fn test_file_upload_multiple_files() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Upload two files
    let resp1 = client
        .post(format!("{}/files:upload", server.local_url))
        .body(b"file one content".to_vec())
        .send()
        .await
        .unwrap();
    let body1: serde_json::Value = resp1.json().await.unwrap();
    let id1 = body1["id"]
        .as_str()
        .ok_or("should have file id")?
        .to_string();

    let resp2 = client
        .post(format!("{}/files:upload", server.local_url))
        .body(b"file two content".to_vec())
        .send()
        .await
        .unwrap();
    let body2: serde_json::Value = resp2.json().await.unwrap();
    let id2 = body2["id"]
        .as_str()
        .ok_or("should have file id")?
        .to_string();

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
    Ok(())
}

#[tokio::test]
async fn test_tasks_send_with_idempotency_key() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // First request with idempotencyKey
    let resp1 = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
            "idempotencyKey": "idem-1"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp1.status(), 200);
    let body1: serde_json::Value = resp1.json().await.unwrap();
    let task_id1 = body1["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Second request with same idempotencyKey should return same task
    let resp2 = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello again"}]},
            "idempotencyKey": "idem-1"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), 200);
    let body2: serde_json::Value = resp2.json().await.unwrap();
    let task_id2 = body2["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    assert_eq!(
        task_id1, task_id2,
        "same idempotencyKey should return the same task"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_tasks_send_with_different_idempotency_keys() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp1 = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "first"}]},
            "idempotencyKey": "key-a"
        }))
        .send()
        .await
        .unwrap();
    let body1: serde_json::Value = resp1.json().await.unwrap();
    let task_id1 = body1["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    let resp2 = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "second"}]},
            "idempotencyKey": "key-b"
        }))
        .send()
        .await
        .unwrap();
    let body2: serde_json::Value = resp2.json().await.unwrap();
    let task_id2 = body2["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    assert_ne!(
        task_id1, task_id2,
        "different idempotencyKeys should create different tasks"
    );

    server.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Multi-turn support: contextId on tasks.send
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_tasks_send_with_context_id() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
            "contextId": "ctx-123"
        }))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["task"]["contextId"], "ctx-123",
        "contextId should be stored"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_send_with_parent_task_id() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
            "parentTaskId": "parent-456"
        }))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["task"]["parentTaskId"], "parent-456",
        "parentTaskId should be stored"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_send_with_context_and_parent() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
            "contextId": "ctx-123",
            "parentTaskId": "parent-456"
        }))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["task"]["contextId"], "ctx-123",
        "contextId should be stored"
    );
    assert_eq!(
        body["task"]["parentTaskId"], "parent-456",
        "parentTaskId should be stored"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_send_ignores_legacy_session_field() {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
            "sessionId": "sess-ignored"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "sessionId must be ignored, not rejected"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["task"].get("sessionId").is_none(),
        "sessionId must not be echoed back on the task"
    );

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// Cache-Control and ETag headers on agent.json (§8.6)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_agent_card_cache_headers() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    let resp = client
        .get(format!("{}/.well-known/agent-card.json", server.local_url))
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
        "agent card should have Cache-Control: max-age=300"
    );

    let etag = resp.headers().get("ETag").and_then(|v| v.to_str().ok());
    assert!(etag.is_some(), "agent card should have an ETag header");
    let etag = etag.ok_or("should have ETag")?;
    assert!(etag.starts_with('"'), "ETag should be quoted");
    // With the test server card, version is "1.0"
    assert_eq!(etag, r#""1.0""#, "ETag should match card version");

    server.shutdown().await;
    Ok(())
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
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();

    let resp = reqwest::get(format!("{}/extendedAgentCard", server.local_url))
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
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {
                "role": "user",
                "parts": [{"type": "unknown_type", "data": "something"}]
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("error").is_some(),
        "Should return error for unsupported content type"
    );
    assert_eq!(body["error"]["code"], 400, "Should return BAD_REQUEST");
    assert_eq!(
        body["error"]["details"][0]["reason"],
        "CONTENT_TYPE_NOT_SUPPORTED"
    );
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("Content type not supported"),
        "Error message should mention content type, got: {:?}",
        body["error"]["message"]
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
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
            "metadata": {"source": "test", "priority": 5}
        }))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["task"]["metadata"]["source"], "test",
        "metadata.source should be stored"
    );
    assert_eq!(
        body["task"]["metadata"]["priority"], 5,
        "metadata.priority should be stored"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_get_returns_metadata() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task with metadata
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
            "metadata": {"source": "test", "key": "value"}
        }))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Get the task and verify metadata is present
    let resp = client
        .get(format!("{}/tasks/{task_id}", server.local_url))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["task"]["metadata"]["source"], "test",
        "metadata should be preserved when getting task"
    );
    assert_eq!(
        body["task"]["metadata"]["key"], "value",
        "metadata should be preserved when getting task"
    );

    server.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// historyLength on tasks.get
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_task_with_history_length_full() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task via send (adds message to history)
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"turn 1"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Get the task with no historyLength (should return full history)
    let resp = client
        .get(format!("{}/tasks/{}", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let history = body["task"]["history"]
        .as_array()
        .ok_or("should have history")?;
    assert_eq!(history.len(), 1, "should have 1 history entry");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_get_task_with_history_length_filter() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;
    let store = server.task_store();

    // Create a task via send (adds first message to history)
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"turn 1"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Append more messages to history directly (simulate multi-turn)
    store
        .append_history(
            &task_id,
            Message {
                role: Role::Agent,
                parts: vec![Part::Text {
                    text: "response 1".into(),
                }],
                message_id: uuid::Uuid::new_v4().to_string(),
                extensions: None,
                metadata: None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    store
        .append_history(
            &task_id,
            Message {
                role: Role::User,
                parts: vec![Part::Text {
                    text: "turn 2".into(),
                }],
                message_id: uuid::Uuid::new_v4().to_string(),
                extensions: None,
                metadata: None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;

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
    let history = body["task"]["history"]
        .as_array()
        .ok_or("should have history")?;
    assert_eq!(history.len(), 1, "historyLength=1 should return 1 entry");
    assert_eq!(
        history[0]["role"], "ROLE_USER",
        "last entry should be the user turn"
    );
    assert_eq!(history[0]["parts"][0]["text"], "turn 2");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_get_task_with_history_length_zero() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;

    // Create a task
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"hello"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

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
        body["task"].get("history").is_none(),
        "history should be absent when historyLength=0"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_get_task_with_history_length_invalid() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server().await;
    let store = server.task_store();

    // Create a task
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"hello"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Append some history
    store
        .append_history(
            &task_id,
            Message {
                role: Role::Agent,
                parts: vec![Part::Text {
                    text: "response".into(),
                }],
                message_id: uuid::Uuid::new_v4().to_string(),
                extensions: None,
                metadata: None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;

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
    let history = body["task"]["history"]
        .as_array()
        .ok_or("should have history")?;
    assert_eq!(history.len(), 2, "full history should be returned");

    server.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// IncomingTask channel with contextId and parentTaskId
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_incoming_task_channel_with_context_and_parent() -> Result<()> {
    ensure_crypto_provider();
    let card = AgentCard {
        name: "test".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let mut server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();
    let mut task_rx = server
        .take_incoming_task_receiver()
        .ok_or("should have incoming task receiver")?;

    let client = test_client();

    // Send a task with contextId and parentTaskId
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "multi-turn"}]},
            "contextId": "ctx-999",
            "parentTaskId": "parent-888"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Verify the event was received with all fields
    let incoming = task_rx.try_recv().map_err(|e| format!("{e:?}"))?;
    assert_eq!(incoming.context_id, Some("ctx-999".into()));
    assert_eq!(incoming.parent_task_id, Some("parent-888".into()));

    server.shutdown().await;
    Ok(())
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
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();
    let client = reqwest::Client::new(); // no A2A-Version header

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]}
        }))
        .send()
        .await
        .unwrap();
    // Version check errors return HTTP 400
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(body["error"]["status"], "INVALID_REQUEST");
    assert_eq!(
        body["error"]["details"][0]["reason"],
        "VERSION_NOT_SUPPORTED"
    );
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
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
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
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(body["error"]["status"], "INVALID_REQUEST");
    assert_eq!(
        body["error"]["details"][0]["reason"],
        "VERSION_NOT_SUPPORTED"
    );

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
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();
    let client = reqwest::Client::new(); // no A2A-Version header

    // /.well-known/agent-card.json should work without A2A-Version
    let resp = client
        .get(format!("{}/.well-known/agent-card.json", server.local_url))
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
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();
    let client = reqwest::Client::new(); // no A2A-Version header

    // /extendedAgentCard should work without A2A-Version
    let resp = client
        .get(format!("{}/extendedAgentCard", server.local_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("agentCard").is_some());

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// Agent card update via agent_card_handle()
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_agent_card_update_via_handle() -> Result<()> {
    ensure_crypto_provider();

    let card = AgentCard {
        name: "Agent A".to_string(),
        description: Some("First agent".to_string()),
        url: "http://127.0.0.1:0".to_string(),
        version: "1.0".to_string(),
        skills: vec![Skill {
            id: "skill-a".into(),
            name: "Skill A".into(),
            description: "First skill".into(),
            inputs: None,
            outputs: None,
        }],
        ..Default::default()
    };
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();
    let client = test_client();

    // GET initial card — assert name is "Agent A"
    let resp = client
        .get(format!("{}/.well-known/agent-card.json", server.local_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "Agent A");
    assert_eq!(body["description"], "First agent");
    let skills = body["skills"]
        .as_array()
        .ok_or("should have skills array")?;
    assert_eq!(skills.len(), 1);

    // Write a new card via agent_card_handle()
    {
        let card_handle = server.agent_card_handle();
        let mut card = card_handle.write().expect("agent_card lock");
        let new_skills = vec![Skill {
            id: "skill-b".into(),
            name: "Skill B".into(),
            description: "Second skill".into(),
            inputs: None,
            outputs: None,
        }];
        *card = rebuild_card_for_switch(&card, "Agent B", Some("Second agent"), new_skills);
    }

    // GET card again — assert name is now "Agent B"
    let resp = client
        .get(format!("{}/.well-known/agent-card.json", server.local_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "Agent B");
    assert_eq!(body["description"], "Second agent");
    let skills = body["skills"]
        .as_array()
        .ok_or("should have skills array")?;
    assert_eq!(skills.len(), 1);
    assert_eq!(body["skills"][0]["name"], "Skill B");

    // Server-bound fields (url, version) are preserved
    // The url stays as the original placeholder since the server doesn't
    // update the card's url field — AgentBuilder does that after start().
    assert_eq!(body["url"], "http://127.0.0.1:0");
    assert_eq!(body["version"], "1.0");

    server.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// A2A-Extensions header validation (spec §3.2.6, §14.2.2)
// ---------------------------------------------------------------------------

/// Start a test server whose agent card declares `extensions`.
async fn test_server_with_extensions(
    extensions: Vec<String>,
) -> Result<(A2aServer, reqwest::Client)> {
    let card = AgentCard {
        name: "test-agent".into(),
        url: "http://127.0.0.1:0".into(),
        version: "1.0".into(),
        extensions,
        ..Default::default()
    };
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .map_err(|e| format!("server should start: {e:?}"))?;
    Ok((server, test_client()))
}

/// POST a minimal `message:send` body with an optional `A2A-Extensions` header.
async fn send_with_extensions(
    client: &reqwest::Client,
    url: &str,
    extensions: Option<&str>,
) -> Result<reqwest::Response> {
    let mut req = client.post(format!("{url}/message:send")).json(&json!({
        "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]}
    }));
    if let Some(value) = extensions {
        req = req.header("A2A-Extensions", value);
    }
    req.send()
        .await
        .map_err(|e| format!("request should succeed: {e:?}").into())
}

#[tokio::test]
async fn test_a2a_extensions_absent_accepted() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server_with_extensions(vec![]).await?;

    let resp = send_with_extensions(&client, &server.local_url, None).await?;
    assert_eq!(resp.status(), 200);

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_a2a_extensions_empty_value_accepted() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server_with_extensions(vec![]).await?;

    let resp = send_with_extensions(&client, &server.local_url, Some("")).await?;
    assert_eq!(resp.status(), 200);

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_a2a_extensions_matching_accepted() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server_with_extensions(vec![
        "https://example.com/ext".into(),
        "https://example.com/ext2".into(),
    ])
    .await?;

    let resp = send_with_extensions(
        &client,
        &server.local_url,
        Some("https://example.com/ext,https://example.com/ext2"),
    )
    .await?;
    assert_eq!(resp.status(), 200);

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_a2a_extensions_whitespace_trimmed() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server_with_extensions(vec![
        "https://example.com/ext".into(),
        "https://example.com/ext2".into(),
    ])
    .await?;

    let resp = send_with_extensions(
        &client,
        &server.local_url,
        Some(" https://example.com/ext , https://example.com/ext2 "),
    )
    .await?;
    assert_eq!(resp.status(), 200);

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_a2a_extensions_unsupported_rejected() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) =
        test_server_with_extensions(vec!["https://example.com/ext".into()]).await?;

    let resp = send_with_extensions(
        &client,
        &server.local_url,
        Some("https://example.com/ext,https://example.com/unknown"),
    )
    .await?;
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("{e:?}"))?;
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(
        body["error"]["details"][0]["reason"],
        "UNSUPPORTED_OPERATION"
    );
    let message = body["error"]["message"]
        .as_str()
        .ok_or("should have message")?;
    assert!(
        message.contains("https://example.com/unknown"),
        "message should name the unsupported extension, got: {message}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_a2a_extensions_empty_card_rejects_any() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server_with_extensions(vec![]).await?;

    let resp =
        send_with_extensions(&client, &server.local_url, Some("https://example.com/ext")).await?;
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        body["error"]["details"][0]["reason"],
        "UNSUPPORTED_OPERATION"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_a2a_extensions_non_a2a_path_skipped() -> Result<()> {
    ensure_crypto_provider();
    let (server, client) = test_server_with_extensions(vec![]).await?;

    let resp = client
        .get(format!("{}/.well-known/agent-card.json", server.local_url))
        .header("A2A-Extensions", "https://example.com/unknown")
        .send()
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(resp.status(), 200);

    server.shutdown().await;
    Ok(())
}
