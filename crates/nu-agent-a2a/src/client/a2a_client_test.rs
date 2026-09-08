use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncWriteExt, BufWriter as TokioBufWriter};

use crate::{
    A2aError, AgentCapabilities, AgentCard, Message, Part, Peer, PeerCache, Role, TaskState,
};

use super::{A2aClient, cancel_task, send_task};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

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
    let client = A2aClient::new().unwrap();
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
    let cache = PeerCache::default();
    cache.add_or_update(Peer {
        name: "alice".into(),
        url: "http://127.0.0.1:8080".into(),
        host: "127.0.0.1".into(),
        port: 8080,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

    let client = A2aClient::new().unwrap();
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
    let server = crate::A2aServer::start(card, Arc::new(PeerCache::default()), 0)
        .await
        .unwrap();
    let client = A2aClient::new().unwrap();
    let url = server.local_url.clone();
    (server, client, url)
}

#[tokio::test]
async fn test_subscribe_task_immediate_terminal() -> Result<()> {
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
    let sent = send_task(&client, &url, msg, None, None)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let canceled = cancel_task(&client, &url, &sent.id)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(canceled.status.state, TaskState::Canceled);

    // Now subscribe — should immediately return the terminal state
    let result = client
        .subscribe_task(&url, &sent.id)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.status.state, TaskState::Canceled);
    assert_eq!(result.id, sent.id);
    Ok(())
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
    let client = A2aClient::new().unwrap();

    let result = client.subscribe_task("http://127.0.0.1:1", "some-id").await;
    assert!(
        matches!(result, Err(A2aError::ConnectionRefused(_))),
        "Expected ConnectionRefused, got: {result:?}"
    );
}

#[tokio::test]
async fn test_subscribe_task_streams_lifecycle() -> Result<()> {
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
    let sent = send_task(&client, &url, msg, None, None)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // Complete the task directly via the server's task store (no HTTP needed)
    server
        .task_store()
        .complete_task(&sent.id, "Task completed successfully")
        .map_err(|e| format!("{e:?}"))?;

    // Give the SSE notification time to propagate
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Subscribe — should get the terminal Completed state
    let result = client
        .subscribe_task(&url, &sent.id)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.status.state, TaskState::Completed);
    assert_eq!(result.id, sent.id);

    server.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// A2A-Version header
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_client_sends_a2a_version_header() -> Result<()> {
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
    let client = A2aClient::new().unwrap();
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
        .map_err(|e| format!("timeout waiting for echo server: {e:?}"))?
        .ok_or("echo server closed channel")?;
    assert_eq!(captured, "1.0", "client should send A2A-Version: 1.0");
    Ok(())
}

// ---------------------------------------------------------------------------
// SSE chunk UTF-8 boundary (raw TCP mini-server)
// ---------------------------------------------------------------------------

/// Regression: the SSE chunk loop dropped whole TCP chunks whose bytes ended
/// mid-multi-byte-UTF-8-char (`if let Ok(s) = std::str::from_utf8(&bytes)`),
/// silently losing event data. The fix buffers raw bytes and decodes complete
/// events only.
///
/// A raw TCP mini-server writes one SSE event in two `write_all` calls with a
/// 2-byte char straddling the TCP write boundary. Chunk 1 is intentionally
/// invalid UTF-8 on its own; only joining both chunks yields a valid event.
#[tokio::test]
async fn test_subscribe_task_sse_chunk_split_mid_utf8_char_no_data_loss() -> Result<()> {
    ensure_crypto_provider();

    // The full event body. `é` (2 bytes in UTF-8) straddles the boundary:
    // chunk 1 ends with its first byte, chunk 2 starts with its second byte.
    let artifact_text = format!("{}é-tail", "a".repeat(8));
    let task_json = serde_json::json!({
        "task": {
            "id": "sse-split-utf8",
            "status": {
                "state": "COMPLETED",
                "timestamp": "2026-01-01T00:00:00Z"
            },
            "artifacts": [
                {
                    "artifactId": "art-split",
                    "parts": [{"text": artifact_text}]
                }
            ]
        }
    });
    let event_body = format!("data: {task_json}\n\n", task_json = task_json);
    let event_bytes = event_body.into_bytes();

    // Find the `é` inside the JSON string: its first byte must not be the
    // last byte of the chunk (it needs a continuation byte in chunk 2).
    let e_pos = event_bytes
        .windows(2)
        .position(|w| w == [0xC3, 0xA9])
        .ok_or("event must contain U+00E9")?;
    // Split so chunk 1 ends with é's first byte (0xC3).
    let split = e_pos + 1;
    assert_eq!(event_bytes[split - 1], 0xC3, "split must be mid-char");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    // Mini-server: accept one connection, upgrade to chunked SSE, then write
    // the event in two separate writes with the multi-byte char straddling
    // the write boundary.
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut stream = TokioBufWriter::new(stream);

        let head = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
        stream.write_all(head).await?;
        stream.flush().await?;

        let (c1, c2) = event_bytes.split_at(split);
        // Chunk 1: mid-char (0xC3 as the final byte)
        stream
            .write_all(format!("{:x}\r\n", c1.len()).as_bytes())
            .await?;
        stream.write_all(c1).await?;
        stream.write_all(b"\r\n").await?;
        stream.flush().await?;

        // Let the client consume chunk 1 before writing chunk 2, so the two
        // writes cannot coalesce into one TCP segment (which would mask the
        // split boundary).
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Chunk 2: the rest of the event + terminal delimiter.
        stream
            .write_all(format!("{:x}\r\n", c2.len()).as_bytes())
            .await?;
        stream.write_all(c2).await?;
        stream.write_all(b"\r\n").await?;
        stream.flush().await?;

        // Terminate chunked encoding.
        stream.write_all(b"0\r\n\r\n").await?;
        stream.flush().await?;
        Ok::<(), std::io::Error>(())
    });

    let url = format!("http://127.0.0.1:{}", addr.port());
    let client = A2aClient::new().unwrap();

    // -- Exec
    let task = client
        .subscribe_task(&url, "sse-split-utf8")
        .await
        .map_err(|e| format!("subscribe should return the terminal task: {e:?}"))?;

    // -- Check
    assert_eq!(task.id, "sse-split-utf8");
    assert_eq!(task.status.state, TaskState::Completed);
    assert_eq!(
        task.artifacts.len(),
        1,
        "the event carrying the split char must not be dropped"
    );
    assert_eq!(
        task.artifacts[0].parts,
        vec![Part::Text {
            text: artifact_text,
        }],
        "multi-byte char must survive the chunk boundary intact"
    );
    Ok(())
}
