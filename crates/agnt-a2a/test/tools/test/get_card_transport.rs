use std::time::Duration;

use super::*;

/// The card the stub serves on the happy path.
const STUB_CARD_JSON: &str =
    r#"{"name":"stub-agent","capabilities":{"streaming":true},"skills":[]}"#;

// ---------------------------------------------------------------------------
// Record-and-reply card stub
// ---------------------------------------------------------------------------

/// One card request as the stub received it.
#[derive(Clone, Debug)]
struct RecordedRequest {
    a2a_version: Option<String>,
}

/// Start a localhost stub that answers the well-known card path with `status`
/// and `body`, recording the headers of every request.
async fn start_card_stub(
    status: u16,
    body: &'static str,
) -> Result<(String, u16, tokio::sync::mpsc::Receiver<RecordedRequest>)> {
    let (tx, rx) = tokio::sync::mpsc::channel::<RecordedRequest>(4);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let app = axum::Router::new().route(
        "/.well-known/agent-card.json",
        axum::routing::get(move |headers: axum::http::HeaderMap| {
            let tx = tx.clone();
            async move {
                let _ = tx
                    .send(RecordedRequest {
                        a2a_version: headers
                            .get("a2a-version")
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_string),
                    })
                    .await;
                (
                    axum::http::StatusCode::from_u16(status).unwrap_or(axum::http::StatusCode::OK),
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    body,
                )
            }
        }),
    );

    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Ok((format!("http://127.0.0.1:{}", addr.port()), addr.port(), rx))
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Build a tool context whose peer cache holds one peer pointing at the stub.
fn stub_tool_ctx(base_url: &str, port: u16) -> Result<A2aToolContext> {
    let ctx = A2aToolContext {
        client: A2aClient::new(crate::test_support::test_transport()),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    ctx.cache.add_or_update(Peer {
        name: "stub-agent".into(),
        url: base_url.to_string(),
        host: "127.0.0.1".into(),
        port,
        card: None,
        discovered_at: std::time::Instant::now(),
    });
    Ok(ctx)
}

/// Drive `agent_getCard` through the same dispatch entry the rig adapter uses.
async fn dispatch_get_card(ctx: &A2aToolContext) -> Result<serde_json::Value> {
    crate::tools::registry::handle_dispatch(
        "agent_getCard",
        ctx,
        serde_json::json!({ "name": "stub-agent" }),
    )
    .await
    .map_err(|e| format!("agent_getCard should succeed: {e}").into())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_agent_get_card_stamps_a2a_version_and_passes_the_card_through() -> Result<()> {
    // -- Setup & Fixtures
    let (base_url, port, mut requests) = start_card_stub(200, STUB_CARD_JSON).await?;
    let ctx = stub_tool_ctx(&base_url, port)?;

    // -- Exec
    let payload = dispatch_get_card(&ctx).await?;

    // -- Check
    let request = tokio::time::timeout(Duration::from_secs(5), requests.recv())
        .await
        .map_err(|_| "the card stub should record the request within 5 s")?
        .ok_or("the card stub should record a request")?;
    assert_eq!(
        request.a2a_version.as_deref(),
        Some(crate::A2A_VERSION),
        "the get-card fetch must ride the shared client's a2a-version header"
    );
    assert_eq!(
        payload,
        serde_json::from_str::<serde_json::Value>(STUB_CARD_JSON)?,
        "get_card must return the card bytes unchanged"
    );
    Ok(())
}

#[tokio::test]
async fn test_agent_get_card_keeps_http_errors_inside_the_payload() -> Result<()> {
    // -- Setup & Fixtures
    let (base_url, port, mut requests) = start_card_stub(500, r#"{"error":"boom"}"#).await?;
    let ctx = stub_tool_ctx(&base_url, port)?;

    // -- Exec
    let payload = dispatch_get_card(&ctx).await?;

    // -- Check
    let _ = tokio::time::timeout(Duration::from_secs(5), requests.recv()).await;
    assert_eq!(
        payload["name"].as_str(),
        Some("stub-agent"),
        "a failed fetch must still name the requested agent"
    );
    let error = payload
        .get("error")
        .and_then(|v| v.as_str())
        .ok_or("a non-2xx card fetch must report an error inside the payload")?;
    assert!(
        error.contains("500"),
        "the payload error must name the HTTP status; got: {error}"
    );
    Ok(())
}
