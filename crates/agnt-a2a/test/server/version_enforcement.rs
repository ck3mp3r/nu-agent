use super::*;

#[tokio::test]
async fn test_tasks_send_unsupported_content_type() {
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

#[tokio::test]
async fn test_a2a_version_missing_rejected() {
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
    crate::discovery::card::ensure_crypto_provider();
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
    crate::discovery::card::ensure_crypto_provider();
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
    crate::discovery::card::ensure_crypto_provider();
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
    crate::discovery::card::ensure_crypto_provider();
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
