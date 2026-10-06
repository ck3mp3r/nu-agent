use super::*;

#[tokio::test]
async fn test_sender_url_populates_peer_cache() {
    let cache = Arc::new(PeerCache::default());
    let card = AgentCard {
        name: "test".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server =
        A2aServer::start_with_blocking_timeout(card, cache.clone(), 0, TEST_BLOCKING_TIMEOUT)
            .await
            .unwrap();

    let client = test_client();

    // Send a task with senderUrl
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {
                "role": "user",
                "parts": [{"type": "text", "text": "hello"}]
            },
            "senderUrl": "http://sender.local:12345"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Verify peer cache has the sender
    let peers = cache.list();
    assert_eq!(peers.len(), 1, "PeerCache should have 1 entry");
    assert_eq!(peers[0].url, "http://sender.local:12345");
    assert_eq!(peers[0].host, "sender.local");
    assert_eq!(peers[0].port, 12345);

    server.shutdown().await;
}

#[tokio::test]
async fn test_sender_url_empty_does_not_populate_cache() {
    let cache = Arc::new(PeerCache::default());
    let card = AgentCard {
        name: "test".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let server =
        A2aServer::start_with_blocking_timeout(card, cache.clone(), 0, TEST_BLOCKING_TIMEOUT)
            .await
            .unwrap();

    let client = test_client();

    // Send a task without senderUrl
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {
                "role": "user",
                "parts": [{"type": "text", "text": "hello"}]
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Verify peer cache is empty
    let peers = cache.list();
    assert_eq!(
        peers.len(),
        0,
        "PeerCache should be empty when no senderUrl"
    );

    server.shutdown().await;
}
