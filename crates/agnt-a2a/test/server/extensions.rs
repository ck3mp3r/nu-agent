use super::*;

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
    let (server, client) = test_server_with_extensions(vec![]).await?;

    let resp = send_with_extensions(&client, &server.local_url, None).await?;
    assert_eq!(resp.status(), 200);

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_a2a_extensions_empty_value_accepted() -> Result<()> {
    let (server, client) = test_server_with_extensions(vec![]).await?;

    let resp = send_with_extensions(&client, &server.local_url, Some("")).await?;
    assert_eq!(resp.status(), 200);

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_a2a_extensions_matching_accepted() -> Result<()> {
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
