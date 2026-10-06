use super::*;

#[tokio::test]
async fn test_server_starts_and_returns_port() {
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
    crate::discovery::card::ensure_crypto_provider();
    let resp = reqwest::Client::new()
        .get(format!("{}/health", server.local_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    assert_eq!(body, "ok");

    server.shutdown().await;
}

#[tokio::test]
async fn test_a2a_version_response_header() {
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

#[tokio::test]
async fn test_agent_card_endpoint() -> Result<()> {
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
async fn test_agent_card_cache_headers() -> Result<()> {
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

#[tokio::test]
async fn test_extended_agent_card() {
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
    let resp = reqwest::Client::new()
        .get(format!("{}/extendedAgentCard", server.local_url))
        .send()
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

#[tokio::test]
async fn test_agent_card_update_via_handle() -> Result<()> {
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
