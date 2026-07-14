use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;

// ---------------------------------------------------------------------------
// Crypto provider
// ---------------------------------------------------------------------------

// The workspace reqwest is built with `rustls-no-provider`, meaning the
// application must install a crypto provider before constructing a Client.
static CRYPTO_INIT: std::sync::Once = std::sync::Once::new();

fn ensure_crypto_provider() {
    CRYPTO_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Start a test server, return (server, client, server_url).
async fn test_setup() -> (A2aServer, A2aClient, String) {
    ensure_crypto_provider();

    let card = AgentCard {
        name: "test-server".into(),
        url: "http://127.0.0.1:0".into(),
        version: "1.0".into(),
        capabilities: AgentCapabilities::default(),
        skills: vec![],
        ..Default::default()
    };
    let server = A2aServer::start(card, Arc::new(PeerCache::new()), 0)
        .await
        .unwrap();
    let client = A2aClient::new();
    let url = server.local_url.clone();
    (server, client, url)
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

    let _ = client.send_task(&url, msg, None, None).await;

    let captured = tokio::time::timeout(Duration::from_secs(2), version_rx.recv())
        .await
        .expect("timeout waiting for echo server")
        .expect("echo server closed channel");
    assert_eq!(captured, "1.0", "client should send A2A-Version: 1.0");
}

// ---------------------------------------------------------------------------
// send_task
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_send_task() {
    let (_server, client, url) = test_setup().await;

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let task = client.send_task(&url, msg, None, None).await.unwrap();

    assert!(!task.id.is_empty(), "Task should have an ID");
    assert_eq!(
        task.status.state,
        TaskState::Working,
        "New task should be in Working state"
    );
    // UUID format: 36 chars
    assert_eq!(task.id.len(), 36, "Task ID should be a UUID");
}

#[tokio::test]
async fn test_send_task_with_session() {
    let (_server, client, url) = test_setup().await;

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let task = client
        .send_task(&url, msg, Some("sess-1".into()), None)
        .await
        .unwrap();
    assert_eq!(task.session_id, Some("sess-1".into()));
}

#[tokio::test]
async fn test_send_task_with_sender_url() {
    let (_server, client, url) = test_setup().await;

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let task = client
        .send_task(&url, msg, None, Some("http://me.local:12345".into()))
        .await
        .unwrap();

    assert!(!task.id.is_empty(), "Task should have an ID");
}

#[tokio::test]
async fn test_send_task_connection_refused() {
    ensure_crypto_provider();
    let client = A2aClient::new();
    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let result = client
        .send_task("http://127.0.0.1:1", msg, None, None)
        .await;
    assert!(
        matches!(result, Err(A2aError::ConnectionRefused(_))),
        "Expected ConnectionRefused, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// get_task
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_task() {
    let (_server, client, url) = test_setup().await;

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    let sent = client.send_task(&url, msg, None, None).await.unwrap();

    let retrieved = client.get_task(&url, &sent.id).await.unwrap();
    assert_eq!(retrieved.id, sent.id);
    assert_eq!(retrieved.status.state, TaskState::Working);
}

#[tokio::test]
async fn test_get_task_not_found() {
    let (_server, client, url) = test_setup().await;
    let result = client.get_task(&url, "nonexistent-id").await;
    assert!(
        matches!(result, Err(A2aError::TaskNotFound(_))),
        "Expected TaskNotFound, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// cancel_task
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_cancel_task() {
    let (_server, client, url) = test_setup().await;

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "cancel me".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    let sent = client.send_task(&url, msg, None, None).await.unwrap();

    let canceled = client.cancel_task(&url, &sent.id).await.unwrap();
    assert_eq!(canceled.status.state, TaskState::Canceled);
}

#[tokio::test]
async fn test_cancel_already_completed() {
    let (_server, client, url) = test_setup().await;

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "cancel me".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    let sent = client.send_task(&url, msg, None, None).await.unwrap();

    // First cancel should succeed
    let first = client.cancel_task(&url, &sent.id).await.unwrap();
    assert_eq!(first.status.state, TaskState::Canceled);

    // Second cancel should fail — already Canceled
    let result = client.cancel_task(&url, &sent.id).await;
    assert!(
        matches!(result, Err(A2aError::InvalidStateTransition { .. })),
        "Second cancel should return InvalidStateTransition, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// get_agent_card
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_agent_card() {
    let (_server, client, url) = test_setup().await;
    let card = client.get_agent_card(&url).await.unwrap();
    assert_eq!(card.name, "test-server");
    assert_eq!(card.version, "1.0");
}

#[tokio::test]
async fn test_get_agent_card_connection_refused() {
    ensure_crypto_provider();
    let client = A2aClient::new();
    let result = client.get_agent_card("http://127.0.0.1:1").await;
    assert!(matches!(result, Err(A2aError::ConnectionRefused(_))));
}

// ---------------------------------------------------------------------------
// subscribe_task
// ---------------------------------------------------------------------------

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
    let sent = client.send_task(&url, msg, None, None).await.unwrap();
    let canceled = client.cancel_task(&url, &sent.id).await.unwrap();
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

    // _server is dropped here (graceful shutdown via Drop)
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
    let sent = client.send_task(&url, msg, None, None).await.unwrap();

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
// list_peers / get_peer
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
        discovered_at: Instant::now(),
    });

    let client = A2aClient::new();
    let peers = client.list_peers(&cache);
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].name, "alice");
}

// ---------------------------------------------------------------------------
// list_tasks
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_list_tasks() {
    let (_server, client, url) = test_setup().await;

    // Send a couple of tasks
    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "first".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    client.send_task(&url, msg, None, None).await.unwrap();

    let msg2 = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "second".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    client.send_task(&url, msg2, None, None).await.unwrap();

    // List all tasks
    let tasks = client.list_tasks(&url, None).await.unwrap();
    assert_eq!(tasks.len(), 2, "Should list 2 tasks");
}

#[tokio::test]
async fn test_list_tasks_filtered() {
    let (_server, client, url) = test_setup().await;

    // Send a task (it starts Submitted then transitions to Working)
    let send_resp = client
        .send_task(
            &url,
            Message {
                role: Role::User,
                parts: vec![Part::Text {
                    text: "to-cancel".into(),
                }],
                message_id: uuid::Uuid::new_v4().to_string(),
                extensions: None,
                metadata: None,
            },
            None,
            None,
        )
        .await
        .unwrap();

    let msg2 = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "keep".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    client.send_task(&url, msg2, None, None).await.unwrap();

    // Cancel the first task so it's in 'canceled' state
    client.cancel_task(&url, &send_resp.id).await.unwrap();

    // List only working tasks
    let working = client
        .list_tasks(&url, Some(TaskState::Working))
        .await
        .unwrap();
    assert_eq!(working.len(), 1, "Should have 1 working task");
    assert_eq!(working[0].status.state, TaskState::Working);

    // List only canceled tasks
    let canceled = client
        .list_tasks(&url, Some(TaskState::Canceled))
        .await
        .unwrap();
    assert_eq!(canceled.len(), 1, "Should have 1 canceled task");
    assert_eq!(canceled[0].status.state, TaskState::Canceled);
}

// ---------------------------------------------------------------------------
// Full lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_full_lifecycle() {
    let (_server, client, url) = test_setup().await;

    // Send
    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "lifecycle".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    let sent = client.send_task(&url, msg, None, None).await.unwrap();
    assert_eq!(sent.status.state, TaskState::Working);

    // Get
    let retrieved = client.get_task(&url, &sent.id).await.unwrap();
    assert_eq!(retrieved.id, sent.id);

    // Cancel
    let canceled = client.cancel_task(&url, &sent.id).await.unwrap();
    assert_eq!(canceled.status.state, TaskState::Canceled);

    // Get after cancel
    let final_task = client.get_task(&url, &sent.id).await.unwrap();
    assert_eq!(final_task.status.state, TaskState::Canceled);
}
