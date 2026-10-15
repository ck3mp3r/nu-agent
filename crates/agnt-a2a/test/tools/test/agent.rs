use super::*;

#[tokio::test]
async fn test_handle_agent_list_empty() -> Result<()> {
    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    let result = handle_agent_list(ctx, serde_json::json!({}))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let agents = result["agents"]
        .as_array()
        .ok_or("should have agents array")?;
    assert_eq!(agents.len(), 0);
    Ok(())
}

#[tokio::test]
async fn test_handle_agent_list_with_peers() -> Result<()> {
    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new().unwrap(),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    ctx.cache.add_or_update(Peer {
        name: "alice".into(),
        url: "http://127.0.0.1:8080".into(),
        host: "127.0.0.1".into(),
        port: 8080,
        card: Some(AgentCard {
            name: "alice".into(),
            description: Some("Alice agent".into()),
            skills: vec![Skill {
                id: "chat".into(),
                name: "Chat".into(),
                description: "Chatting".into(),
                inputs: None,
                outputs: None,
            }],
            ..Default::default()
        }),
        discovered_at: std::time::Instant::now(),
    });
    let result = handle_agent_list(ctx, serde_json::json!({}))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let agents = result["agents"]
        .as_array()
        .ok_or("should have agents array")?;
    assert_eq!(agents.len(), 1);
    assert_eq!(result["agents"][0]["name"], "alice");
    Ok(())
}

#[tokio::test]
async fn test_handle_agent_get_card_not_found() {
    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new().unwrap(),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    let params = serde_json::json!({"name": "nonexistent"});
    let result = handle_agent_get_card(ctx, params).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found"));
}
