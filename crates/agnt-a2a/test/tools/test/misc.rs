use super::*;

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
    let server = test_server().await;

    let (completion_tx, mut completion_rx) = tokio::sync::mpsc::channel::<A2aCompletionEvent>(16);
    let mut ctx = tool_ctx(&server);
    ctx.completion_tx = Some(completion_tx);
    ctx.runtime_handle = Some(tokio::runtime::Handle::current());

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

    // Complete the task so the watcher observes a terminal state and emits the
    // completion event. The watcher falls back to `get_task` when the task is
    // already terminal at subscribe time.
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
