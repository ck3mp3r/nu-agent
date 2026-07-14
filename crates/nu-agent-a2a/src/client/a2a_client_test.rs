use std::sync::Arc;
use std::time::Duration;

use crate::{
    A2aError, AgentCapabilities, AgentCard, Message, Part, Peer, PeerCache, Role, TaskState,
};

use super::{A2aClient, cancel_task, send_task};

// The workspace reqwest is built with `rustls-no-provider`, meaning the
// application must install a crypto provider before constructing a Client.
static CRYPTO_INIT: std::sync::Once = std::sync::Once::new();

fn ensure_crypto_provider() {
    CRYPTO_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

// ---------------------------------------------------------------------------
// A2aClient construction
// ---------------------------------------------------------------------------

#[test]
fn test_default_client() {
    ensure_crypto_provider();
    let client = A2aClient::new();
    // Just verify it doesn't panic
    let _ = client;
}

#[test]
fn test_default_trait() {
    ensure_crypto_provider();
    let client = A2aClient::default();
    let _ = client;
}

// ---------------------------------------------------------------------------
// list_peers / get_peer (synchronous cache methods)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_list_peers() {
    ensure_crypto_provider();
    let cache = PeerCache::new();
    cache.add_or_update(Peer {
        name: "alice".into(),
        url: "http://127.0.0.1:8080".into(),
        host: "127.0.0.1".into(),
        port: 8080,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

    let client = A2aClient::new();
    let peers = client.list_peers(&cache);
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].name, "alice");
}

// ---------------------------------------------------------------------------
// subscribe_task with real server
// ---------------------------------------------------------------------------

async fn test_setup() -> (crate::A2aServer, A2aClient, String) {
    ensure_crypto_provider();

    let card = AgentCard {
        name: "test-server".into(),
        url: "http://127.0.0.1:0".into(),
        version: "1.0".into(),
        capabilities: AgentCapabilities::default(),
        skills: vec![],
        ..Default::default()
    };
    let server = crate::A2aServer::start(card, Arc::new(PeerCache::new()), 0)
        .await
        .unwrap();
    let client = A2aClient::new();
    let url = server.local_url.clone();
    (server, client, url)
}

#[tokio::test]
async fn test_subscribe_task_immediate_terminal() {
    let (_server, client, url) = test_setup().await;

    // Send a task, then cancel it so it's in a terminal state
    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "subscribe-test".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    let sent = send_task(&client, &url, msg, None, None).await.unwrap();
    let canceled = cancel_task(&client, &url, &sent.id).await.unwrap();
    assert_eq!(canceled.status.state, TaskState::Canceled);

    // Now subscribe — should immediately return the terminal state
    let result = client.subscribe_task(&url, &sent.id).await.unwrap();
    assert_eq!(result.status.state, TaskState::Canceled);
    assert_eq!(result.id, sent.id);
}

#[tokio::test]
async fn test_subscribe_task_not_found() {
    let (_server, client, url) = test_setup().await;

    let result = client.subscribe_task(&url, "nonexistent-id").await;
    assert!(
        matches!(result, Err(A2aError::TaskNotFound(_))),
        "Expected TaskNotFound, got: {result:?}"
    );
}

#[tokio::test]
async fn test_subscribe_task_connection_refused() {
    ensure_crypto_provider();
    let client = A2aClient::new();

    let result = client.subscribe_task("http://127.0.0.1:1", "some-id").await;
    assert!(
        matches!(result, Err(A2aError::ConnectionRefused(_))),
        "Expected ConnectionRefused, got: {result:?}"
    );
}

#[tokio::test]
async fn test_subscribe_task_streams_lifecycle() {
    let (server, client, url) = test_setup().await;

    // Send a task
    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "lifecycle-test".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    let sent = send_task(&client, &url, msg, None, None).await.unwrap();

    // Complete the task directly via the server's task store (no HTTP needed)
    server
        .task_store()
        .complete_task(&sent.id, "Task completed successfully")
        .expect("Working → Completed should succeed");

    // Give the SSE notification time to propagate
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Subscribe — should get the terminal Completed state
    let result = client.subscribe_task(&url, &sent.id).await.unwrap();
    assert_eq!(result.status.state, TaskState::Completed);
    assert_eq!(result.id, sent.id);

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// A2A-Version header
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_client_sends_a2a_version_header() {
    ensure_crypto_provider();

    // Mini echo server that captures the A2A-Version request header
    let (version_tx, mut version_rx) = tokio::sync::mpsc::channel::<String>(1);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let app = axum::Router::new().route(
        "/message:send",
        axum::routing::post(move |headers: axum::http::HeaderMap| async move {
            let version = headers
                .get("A2A-Version")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("no-version")
                .to_string();
            let _ = version_tx.send(version).await;
            axum::Json(serde_json::json!({
                "task": {
                    "id": "00000000-0000-0000-0000-000000000000",
                    "status": {
                        "state": "WORKING",
                        "timestamp": "2026-01-01T00:00:00Z"
                    },
                    "artifacts": []
                }
            }))
        }),
    );

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("http://127.0.0.1:{}", addr.port());
    let client = A2aClient::new();
    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "header-test".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let _ = send_task(&client, &url, msg, None, None).await;

    let captured = tokio::time::timeout(Duration::from_secs(2), version_rx.recv())
        .await
        .expect("timeout waiting for echo server")
        .expect("echo server closed channel");
    assert_eq!(captured, "1.0", "client should send A2A-Version: 1.0");
}
