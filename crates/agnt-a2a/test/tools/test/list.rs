use super::*;

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
