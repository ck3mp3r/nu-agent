use std::sync::Arc;
use std::time::Duration;

use crate::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Create a [`reqwest::Client`] that sends `A2A-Version: 1.0` on every
/// request, matching what the middleware expects on A2A API paths.
pub fn test_client() -> reqwest::Client {
    crate::discovery::card::ensure_crypto_provider();
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
pub fn sse_data_events(text: &str) -> Vec<serde_json::Value> {
    text.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str(data).ok())
        .collect()
}

/// Assert the error metadata `timestamp` uses the spec §5.6.1 format
/// `YYYY-MM-DDTHH:mm:ss.sssZ` — a `Z` suffix, never a `+00:00` offset.
pub fn assert_timestamp_has_z_suffix(body: &serde_json::Value) {
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
pub fn assert_task_timestamps_have_z_suffix(task: &serde_json::Value) {
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

pub async fn test_server() -> (A2aServer, reqwest::Client) {
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

/// Start a server whose blocking deadline is short enough for tests.
pub async fn test_server_with_blocking_timeout(timeout: Duration) -> (A2aServer, reqwest::Client) {
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

/// Start a server whose card advertises the given `defaultOutputModes`.
pub async fn test_server_with_output_modes(modes: Vec<String>) -> (A2aServer, reqwest::Client) {
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

/// Receiver end of the local webhook test server's payload channel.
pub type WebhookRx = tokio::sync::mpsc::Receiver<serde_json::Value>;

/// Start a local webhook receiver that forwards each received JSON body to the
/// returned channel. Returns the receiver's base URL.
pub async fn start_webhook_receiver() -> Result<(String, WebhookRx)> {
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
