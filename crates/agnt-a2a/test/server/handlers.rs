use super::*;

// ---------------------------------------------------------------------------
// message:send — request validation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_send_missing_role_returns_400() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"parts": [{"type": "text", "text": "hi"}]}}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(body["error"]["details"][0]["reason"], "INVALID_ARGUMENT");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("role"),
        "error must name the missing field, got: {body}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_missing_parts_returns_400() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user"}}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(body["error"]["details"][0]["reason"], "INVALID_ARGUMENT");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("parts"),
        "error must name the missing field, got: {body}"
    );

    server.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// message:stream — request validation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_send_stream_missing_role_returns_400() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({"message": {"parts": [{"type": "text", "text": "hi"}]}}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(body["error"]["details"][0]["reason"], "INVALID_ARGUMENT");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("role"),
        "error must name the missing field, got: {body}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_stream_missing_parts_returns_400() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({"message": {"role": "user"}}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(body["error"]["details"][0]["reason"], "INVALID_ARGUMENT");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("parts"),
        "error must name the missing field, got: {body}"
    );

    server.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// message:stream — peer cache and artifact forwarding
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_send_stream_sender_url_populates_peer_cache() -> Result<()> {
    let cache = Arc::new(PeerCache::default());
    let card = AgentCard {
        name: "stream-sender".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server =
        A2aServer::start_with_blocking_timeout(card, cache.clone(), 0, TEST_BLOCKING_TIMEOUT)
            .await
            .map_err(|e| format!("{e:?}"))?;
    let client = test_client();

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
            "senderUrl": "http://stream-sender.local:4321"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let peers = cache.list();
    assert_eq!(
        peers.len(),
        1,
        "stream senderUrl must populate the peer cache, got: {peers:?}"
    );
    assert_eq!(peers[0].url, "http://stream-sender.local:4321");
    assert_eq!(peers[0].host, "stream-sender.local");
    assert_eq!(peers[0].port, 4321);

    // Close the stream so the server-side task can exit.
    let task_id = wait_for_task_in_state(
        &client,
        &server.local_url,
        "TASK_STATE_WORKING",
        Duration::from_secs(5),
    )
    .await?;
    client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), resp.bytes()).await;

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_stream_forwards_artifact_events() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "stream"}]},
            "contextId": "ctx-art"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let task_id = wait_for_task_in_state(
        &client,
        &server.local_url,
        "TASK_STATE_WORKING",
        Duration::from_secs(5),
    )
    .await?;

    server
        .task_store()
        .add_artifact(
            &task_id,
            Artifact {
                artifact_id: "art-stream".to_string(),
                name: Some("result".to_string()),
                parts: vec![Part::Text {
                    text: "output".into(),
                }],
                metadata: None,
            },
        )
        .map_err(|e| format!("add_artifact should succeed: {e:?}"))?;

    client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    let body = tokio::time::timeout(Duration::from_secs(5), resp.bytes())
        .await
        .map_err(|_| "SSE stream should close after cancel")?
        .map_err(|e| format!("SSE body read failed: {e:?}"))?;
    let text = String::from_utf8_lossy(&body);

    let events = sse_data_events(&text);
    let artifact_updates: Vec<&serde_json::Value> = events
        .iter()
        .filter_map(|e| e.get("artifactUpdate"))
        .collect();
    assert!(
        !artifact_updates.is_empty(),
        "message:stream must forward artifactUpdate events, got: {text}"
    );
    assert_eq!(artifact_updates[0]["taskId"], task_id);
    assert_eq!(artifact_updates[0]["contextId"], "ctx-art");

    server.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Agent card — invalid ETag version
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_agent_card_invalid_version_returns_500() -> Result<()> {
    // A version containing a control character cannot be parsed into an ETag
    // header value, so the card endpoint must report a 500 rather than panic.
    let card = AgentCard {
        name: "bad-version".into(),
        url: "http://127.0.0.1:0".into(),
        version: "bad\nversion".into(),
        ..Default::default()
    };
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .map_err(|e| format!("{e:?}"))?;
    let client = test_client();

    let resp = client
        .get(format!("{}/.well-known/agent-card.json", server.local_url))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        500,
        "a version that cannot form an ETag must yield 500"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["error"].as_str().unwrap_or("").contains("ETag"),
        "error must mention the invalid ETag, got: {body}"
    );

    server.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Task cancel — no cancel receiver attached
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_task_cancel_without_receiver_still_cancels() -> Result<()> {
    let (mut server, client) = test_server().await;

    // Drop the cancel receiver so the handler's signal send fails and takes
    // the warn path instead of propagating an error.
    let rx = server.take_task_cancel_receiver();
    assert!(rx.is_some(), "server should expose the cancel receiver");
    drop(rx);

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
            "configuration": {"returnImmediately": true}
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    let resp = client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "cancel must succeed even when no receiver is attached"
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["task"]["status"]["state"], "TASK_STATE_CANCELED");

    server.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Server — explicit port binding
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_server_binds_explicit_port() -> Result<()> {
    // Reserve a free port, then release it so the server can bind it.
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = probe.local_addr()?.port();
    drop(probe);

    let card = AgentCard {
        name: "explicit-port".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        port,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .map_err(|e| format!("{e:?}"))?;

    assert_eq!(server.port, port, "server must bind the requested port");
    assert_eq!(server.local_url, format!("http://127.0.0.1:{port}"));

    let client = test_client();
    let resp = client
        .get(format!("{}/health", server.local_url))
        .send()
        .await?;
    assert_eq!(resp.status(), 200);

    server.shutdown().await;
    Ok(())
}
