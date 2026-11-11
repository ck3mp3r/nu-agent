use super::*;

#[tokio::test]
async fn test_handle_tasks_send_missing_param() {
    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new(crate::test_support::test_transport()),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    let params = serde_json::json!({"target": "someone"});
    let result = Tool::Send.handle(ctx, params).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_handle_tasks_send_to_real_server() -> Result<()> {
    // Start real server, add to cache, send task via handler
    let server = test_server().await?;
    let ctx = tool_ctx(&server);

    let params = serde_json::json!({"target": "test-agent", "text": "Hello!"});
    let result = Tool::Send
        .handle(ctx.clone(), params)
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
    let server = test_server().await?;
    let ctx = tool_ctx(&server);

    // -- Exec
    let params = serde_json::json!({
        "target": "test-agent",
        "text": "Hello!",
        "contextId": "ctx-abc"
    });
    let result = Tool::Send
        .handle(ctx.clone(), params)
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
    let server = test_server().await?;
    let ctx = tool_ctx(&server);

    // -- Exec
    let params = serde_json::json!({"target": "test-agent", "text": "Hello!"});
    let result = Tool::Send
        .handle(ctx.clone(), params)
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
async fn test_handle_tasks_send_omitted_contexts_mint_two_distinct_lineages() -> Result<()> {
    let server = test_server().await?;
    let ctx = tool_ctx(&server);

    // -- Exec: two sends that omit the param.
    let first = Tool::Send
        .handle(
            ctx.clone(),
            serde_json::json!({"target": "test-agent", "text": "ALPHA"}),
        )
        .await
        .map_err(|e| format!("{e:?}"))?;
    let second = Tool::Send
        .handle(
            ctx,
            serde_json::json!({"target": "test-agent", "text": "BRAVO"}),
        )
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check: the boundary minted one fresh context per send, so no two
    // sends share a lineage.
    let first_id = first["contextId"]
        .as_str()
        .ok_or("an omitted contextId must echo the minted one")?;
    let second_id = second["contextId"]
        .as_str()
        .ok_or("an omitted contextId must echo the minted one")?;
    assert_ne!(
        first_id, second_id,
        "two omitted sends must mint two distinct lineages"
    );
    assert!(uuid::Uuid::parse_str(first_id).is_ok());
    assert!(uuid::Uuid::parse_str(second_id).is_ok());

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_send_with_own_card_url() -> Result<()> {
    let server = test_server().await?;
    let ctx = tool_ctx_with(
        &server,
        AgentCard {
            name: "sender".into(),
            url: "http://sender.local:34567".into(),
            ..Default::default()
        },
    );

    let params = serde_json::json!({"target": "test-agent", "text": "Hello!"});
    let result = Tool::Send
        .handle(ctx.clone(), params)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(result.get("taskId").is_some(), "Should have a taskId");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_get_to_real_server() -> Result<()> {
    let server = test_server().await?;
    let ctx = tool_ctx(&server);

    // First send a task
    let send_params = serde_json::json!({"target": "test-agent", "text": "Hello!"});
    let send_result = Tool::Send
        .handle(ctx.clone(), send_params)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let task_id = send_result["taskId"]
        .as_str()
        .ok_or("should have taskId string")?
        .to_string();

    // Then get it
    let get_params = serde_json::json!({"target": "test-agent", "taskId": task_id});
    let get_result = Tool::Get
        .handle(ctx, get_params)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(get_result["taskId"], task_id);

    server.shutdown().await;
    Ok(())
}
