//! The `a2a-version` header on every client request, over a bare transport.
//!
//! The production transport is built by the composition root and carries no
//! default headers, so the client must stamp the protocol version itself.

use std::time::Duration;

use crate::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Record-and-reply stub
// ---------------------------------------------------------------------------

/// One request as the stub received it.
#[derive(Clone, Debug)]
struct RecordedRequest {
    path: String,
    a2a_version: Option<String>,
}

/// Start a localhost stub that records every request and answers by route.
async fn start_stub() -> Result<(String, tokio::sync::mpsc::Receiver<RecordedRequest>)> {
    let (tx, rx) = tokio::sync::mpsc::channel::<RecordedRequest>(8);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    fn record(
        tx: &tokio::sync::mpsc::Sender<RecordedRequest>,
        path: &str,
        headers: &axum::http::HeaderMap,
    ) {
        let _ = tx.try_send(RecordedRequest {
            path: path.to_string(),
            a2a_version: headers
                .get("a2a-version")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string),
        });
    }

    let post_tx = tx.clone();
    let get_tx = tx.clone();
    let subscribe_tx = tx.clone();

    let app = axum::Router::new()
        .route(
            "/echo",
            axum::routing::post(
                move |headers: axum::http::HeaderMap, body: axum::Json<serde_json::Value>| {
                    let post_tx = post_tx.clone();
                    async move {
                        record(&post_tx, "/echo", &headers);
                        axum::Json(body.0)
                    }
                },
            ),
        )
        .route(
            "/bytes",
            axum::routing::get(move |headers: axum::http::HeaderMap| {
                let get_tx = get_tx.clone();
                async move {
                    record(&get_tx, "/bytes", &headers);
                    "payload"
                }
            }),
        )
        .route(
            "/tasks/{id}/subscribe",
            axum::routing::post(move |headers: axum::http::HeaderMap| {
                let subscribe_tx = subscribe_tx.clone();
                async move {
                    record(&subscribe_tx, "/tasks/id/subscribe", &headers);
                    (
                        axum::http::StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({
                            "error": { "code": 400, "status": "BAD_REQUEST", "message": "no stream" }
                        })),
                    )
                }
            }),
        );

    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Ok((format!("http://127.0.0.1:{}", addr.port()), rx))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_client_stamps_a2a_version_on_every_request_over_a_bare_transport() -> Result<()> {
    // -- Setup & Fixtures
    let (base_url, mut requests) = start_stub().await?;
    let client = A2aClient::new(crate::test_support::test_transport());

    // -- Exec
    client
        .post_json(
            &format!("{base_url}/echo"),
            serde_json::json!({ "ping": 1 }),
        )
        .await
        .map_err(|e| format!("post_json should succeed: {e:?}"))?;
    let bytes = client
        .get_bytes(&format!("{base_url}/bytes"))
        .await
        .map_err(|e| format!("get_bytes should succeed: {e:?}"))?;
    let _ = client
        .get_bytes_with_timeout(&format!("{base_url}/bytes"), Duration::from_secs(5))
        .await
        .map_err(|e| format!("get_bytes_with_timeout should succeed: {e:?}"))?;
    // A non-2xx subscribe reply still shows what the request carried.
    let subscribe = client.subscribe_task(&base_url, "id").await;
    assert!(
        subscribe.is_err(),
        "the stub answers 400, so subscribe must fail"
    );

    // -- Check
    assert_eq!(bytes, b"payload".to_vec());

    let mut seen = Vec::new();
    for _ in 0..4 {
        let request = tokio::time::timeout(Duration::from_secs(5), requests.recv())
            .await
            .map_err(|_| "the stub should record four requests within 5 s")?
            .ok_or("the stub channel must stay open")?;
        assert_eq!(
            request.a2a_version.as_deref(),
            Some(A2A_VERSION),
            "{} must carry the protocol version",
            request.path
        );
        seen.push(request.path);
    }
    seen.sort();
    assert_eq!(
        seen,
        vec![
            "/bytes".to_string(),
            "/bytes".to_string(),
            "/echo".to_string(),
            "/tasks/id/subscribe".to_string()
        ]
    );
    Ok(())
}
