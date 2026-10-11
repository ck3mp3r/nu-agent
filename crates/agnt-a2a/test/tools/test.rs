use std::sync::Arc;

use crate::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Handler tests
// ---------------------------------------------------------------------------

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
    let card = AgentCard {
        name: "test-agent".into(),
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

    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new().unwrap(),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    ctx.cache.add_or_update(Peer {
        name: "test-agent".into(),
        url: server.local_url.clone(),
        host: "127.0.0.1".into(),
        port: server.port,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

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
    let card = AgentCard {
        name: "test-agent".into(),
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

    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new().unwrap(),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    ctx.cache.add_or_update(Peer {
        name: "test-agent".into(),
        url: server.local_url.clone(),
        host: "127.0.0.1".into(),
        port: server.port,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

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
    let card = AgentCard {
        name: "test-agent".into(),
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

    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new().unwrap(),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    ctx.cache.add_or_update(Peer {
        name: "test-agent".into(),
        url: server.local_url.clone(),
        host: "127.0.0.1".into(),
        port: server.port,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

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
    let card = AgentCard {
        name: "test-agent".into(),
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

    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard {
            name: "sender".into(),
            url: "http://sender.local:34567".into(),
            ..Default::default()
        },
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    ctx.cache.add_or_update(Peer {
        name: "test-agent".into(),
        url: server.local_url.clone(),
        host: "127.0.0.1".into(),
        port: server.port,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

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
    let card = AgentCard {
        name: "test-agent".into(),
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

    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new().unwrap(),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    ctx.cache.add_or_update(Peer {
        name: "test-agent".into(),
        url: server.local_url.clone(),
        host: "127.0.0.1".into(),
        port: server.port,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

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

// ---------------------------------------------------------------------------
// tasks.list handler
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_handle_tasks_list_with_local_store() -> Result<()> {
    let store = Arc::new(InMemoryTaskStore::default());
    store.create_task(None, None, None);
    store.create_task(None, None, None);

    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard::default(),
        task_store: Some(store),
        completion_tx: None,
        runtime_handle: None,
    };

    let result = handle_tasks_list(ctx, serde_json::json!({}))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let tasks = result["tasks"]
        .as_array()
        .ok_or("should have tasks array")?;
    assert_eq!(tasks.len(), 2, "Should list 2 tasks from local store");
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_list_with_local_store_filtered() -> Result<()> {
    let store = Arc::new(InMemoryTaskStore::default());
    let t1 = store.create_task(None, None, None);
    store
        .update_status(&t1.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    store.create_task(None, None, None);

    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard::default(),
        task_store: Some(store),
        completion_tx: None,
        runtime_handle: None,
    };

    let result = handle_tasks_list(ctx, serde_json::json!({"status": "TASK_STATE_WORKING"}))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let tasks = result["tasks"]
        .as_array()
        .ok_or("should have tasks array")?;
    assert_eq!(tasks.len(), 1, "Should list 1 working task");
    assert_eq!(tasks[0]["status"]["state"], "TASK_STATE_WORKING");
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_list_with_local_store_filtered_auth_required() -> Result<()> {
    let store = Arc::new(InMemoryTaskStore::default());
    let t1 = store.create_task(None, None, None);
    store
        .update_status(&t1.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    store.create_task(None, None, None);

    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard::default(),
        task_store: Some(store),
        completion_tx: None,
        runtime_handle: None,
    };

    // No task is in AuthRequired, but the filter must parse and apply — an
    // unparseable status would return Err instead of an empty list.
    let result = handle_tasks_list(
        ctx,
        serde_json::json!({"status": "TASK_STATE_AUTH_REQUIRED"}),
    )
    .await
    .map_err(|e| format!("{e:?}"))?;
    let tasks = result["tasks"]
        .as_array()
        .ok_or("should have tasks array")?;
    assert!(
        tasks.is_empty(),
        "AuthRequired filter should match no tasks, got {tasks:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_list_invalid_status_returns_error() -> Result<()> {
    let store = Arc::new(InMemoryTaskStore::default());
    store.create_task(None, None, None);

    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard::default(),
        task_store: Some(store),
        completion_tx: None,
        runtime_handle: None,
    };

    let result = handle_tasks_list(ctx, serde_json::json!({"status": "INVALID_BOGUS"})).await;
    let err = result
        .err()
        .ok_or("invalid status should return an error")?;
    assert!(
        err.contains("Invalid status") && err.contains("INVALID_BOGUS"),
        "error should name the invalid status, got: {err}"
    );
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_list_no_status_returns_all() -> Result<()> {
    let store = Arc::new(InMemoryTaskStore::default());
    let t1 = store.create_task(None, None, None);
    store
        .update_status(&t1.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    store.create_task(None, None, None);

    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard::default(),
        task_store: Some(store),
        completion_tx: None,
        runtime_handle: None,
    };

    let result = handle_tasks_list(ctx, serde_json::json!({}))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let tasks = result["tasks"]
        .as_array()
        .ok_or("should have tasks array")?;
    assert_eq!(tasks.len(), 2, "Should list all tasks when no status given");
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_list_no_store() -> Result<()> {
    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };

    let result = handle_tasks_list(ctx, serde_json::json!({}))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let tasks = result["tasks"]
        .as_array()
        .ok_or("should have tasks array")?;
    assert!(tasks.is_empty(), "Should return empty list when no store");
    Ok(())
}

#[tokio::test]
async fn agent_list_excludes_self() -> Result<()> {
    let own_url = "http://127.0.0.1:9999".to_string();
    let ctx = A2aToolContext {
        client: A2aClient::new().unwrap(),
        cache: Arc::new(PeerCache::default()),
        own_card: AgentCard {
            name: "self-agent".into(),
            url: own_url.clone(),
            ..Default::default()
        },
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };

    // Peer with same URL → should be excluded from output
    ctx.cache.add_or_update(Peer {
        name: "self-agent".into(),
        url: own_url.clone(),
        host: "127.0.0.1".into(),
        port: 9999,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

    // Peer with different URL → should appear in output
    ctx.cache.add_or_update(Peer {
        name: "other-agent".into(),
        url: "http://127.0.0.1:8888".into(),
        host: "127.0.0.1".into(),
        port: 8888,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

    let result = handle_agent_list(ctx, serde_json::json!({}))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let agents = result["agents"]
        .as_array()
        .ok_or("should have agents array")?;

    // Self should be excluded — only the other agent remains
    assert_eq!(agents.len(), 1, "self-agent should be excluded from output");

    let other_agent = &agents[0];
    assert_eq!(other_agent["name"], "other-agent");

    // is_self field must NOT be present in any entry
    for agent in agents {
        let obj = agent.as_object().ok_or("should be an object")?;
        assert!(
            !obj.contains_key("is_self"),
            "no entry should have an is_self field"
        );
    }
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_send_completion_event_carries_context_id() -> Result<()> {
    let card = AgentCard {
        name: "test-agent".into(),
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

    let (completion_tx, mut completion_rx) = tokio::sync::mpsc::channel::<A2aCompletionEvent>(16);
    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new().unwrap(),
        own_card: AgentCard::default(),
        task_store: None,
        completion_tx: Some(completion_tx),
        runtime_handle: Some(tokio::runtime::Handle::current()),
    };
    ctx.cache.add_or_update(Peer {
        name: "test-agent".into(),
        url: server.local_url.clone(),
        host: "127.0.0.1".into(),
        port: server.port,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

    // -- Exec
    let params = serde_json::json!({
        "target": "test-agent",
        "text": "Hello!",
        "contextId": "ctx-abc"
    });
    let result = handle_tasks_send(ctx.clone(), params)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let task_id = result["taskId"]
        .as_str()
        .ok_or("should have taskId")?
        .to_string();

    // Let the background SSE watcher subscribe, then complete the task so the
    // watcher observes a terminal state and emits the completion event.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    server
        .task_store()
        .update_status(&task_id, TaskState::Completed, None)
        .map_err(|e| format!("update_status should succeed: {e:?}"))?;

    let event = tokio::time::timeout(std::time::Duration::from_secs(5), completion_rx.recv())
        .await
        .map_err(|_| "completion event should arrive")?
        .ok_or("completion channel should not close")?;

    // -- Check
    assert_eq!(
        event.context_id.as_deref(),
        Some("ctx-abc"),
        "completion event must carry the task's contextId"
    );

    server.shutdown().await;
    Ok(())
}
