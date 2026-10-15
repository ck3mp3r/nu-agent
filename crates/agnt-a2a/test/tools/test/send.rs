use super::*;

#[tokio::test]
async fn test_handle_tasks_send_missing_param() {
    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new().unwrap(),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    let params = serde_json::json!({"target": "someone"});
    let result = handle_tasks_send(ctx, params).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_handle_tasks_send_to_real_server() -> Result<()> {
    // Start real server, add to cache, send task via handler
    let server = test_server().await;
    let ctx = tool_ctx(&server);

    let params = serde_json::json!({"target": "test-agent", "text": "Hello!"});
    let result = handle_tasks_send(ctx.clone(), params)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(result.get("taskId").is_some(), "Should have a taskId");
    assert_eq!(
        result["status"], "sent",
        "tasks.send now returns status 'sent' (fire-and-forget)"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_send_with_context_id_returns_context_id() -> Result<()> {
    let server = test_server().await;
    let ctx = tool_ctx(&server);

    // -- Exec
    let params = serde_json::json!({
        "target": "test-agent",
        "text": "Hello!",
        "contextId": "ctx-abc"
    });
    let result = handle_tasks_send(ctx.clone(), params)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(
        result["contextId"], "ctx-abc",
        "tool result should echo the server-assigned contextId"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_send_without_context_id_returns_uuid() -> Result<()> {
    let server = test_server().await;
    let ctx = tool_ctx(&server);

    // -- Exec
    let params = serde_json::json!({"target": "test-agent", "text": "Hello!"});
    let result = handle_tasks_send(ctx.clone(), params)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert!(
        result.get("contextId").is_some(),
        "tool result should always carry a contextId key"
    );
    assert!(
        result["contextId"].is_string(),
        "contextId should be a string when not provided, got: {}",
        result["contextId"]
    );
    let context_id = result["contextId"]
        .as_str()
        .ok_or("contextId should be a string")?;
    assert!(
        uuid::Uuid::parse_str(context_id).is_ok(),
        "contextId should be a UUID when not provided, got: {context_id}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_send_with_own_card_url() -> Result<()> {
    let server = test_server().await;
    let ctx = tool_ctx_with(
        &server,
        AgentCard {
            name: "sender".into(),
            url: "http://sender.local:34567".into(),
            ..Default::default()
        },
    );

    let params = serde_json::json!({"target": "test-agent", "text": "Hello!"});
    let result = handle_tasks_send(ctx.clone(), params)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(result.get("taskId").is_some(), "Should have a taskId");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_get_to_real_server() -> Result<()> {
    let server = test_server().await;
    let ctx = tool_ctx(&server);

    // First send a task
    let send_params = serde_json::json!({"target": "test-agent", "text": "Hello!"});
    let send_result = handle_tasks_send(ctx.clone(), send_params)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let task_id = send_result["taskId"]
        .as_str()
        .ok_or("should have taskId string")?
        .to_string();

    // Then get it
    let get_params = serde_json::json!({"target": "test-agent", "taskId": task_id});
    let get_result = handle_tasks_get(ctx, get_params)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(get_result["taskId"], task_id);

    server.shutdown().await;
    Ok(())
}
