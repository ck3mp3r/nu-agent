use super::*;

#[tokio::test]
async fn test_get_task_with_history_length_full() -> Result<()> {
    let (server, client) = test_server().await;

    // Create a task via send (adds message to history)
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"turn 1"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Get the task with no historyLength (should return full history)
    let resp = client
        .get(format!("{}/tasks/{}", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let history = body["task"]["history"]
        .as_array()
        .ok_or("should have history")?;
    assert_eq!(history.len(), 1, "should have 1 history entry");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_get_task_with_history_length_filter() -> Result<()> {
    let (server, client) = test_server().await;
    let store = server.task_store();

    // Create a task via send (adds first message to history)
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"turn 1"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Append more messages to history directly (simulate multi-turn)
    store
        .append_history(
            &task_id,
            Message {
                role: Role::Agent,
                parts: vec![Part::Text {
                    text: "response 1".into(),
                }],
                message_id: uuid::Uuid::new_v4().to_string(),
                extensions: None,
                metadata: None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    store
        .append_history(
            &task_id,
            Message {
                role: Role::User,
                parts: vec![Part::Text {
                    text: "turn 2".into(),
                }],
                message_id: uuid::Uuid::new_v4().to_string(),
                extensions: None,
                metadata: None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;

    // Get the task with historyLength=1 (should return only last entry)
    let resp = client
        .get(format!(
            "{}/tasks/{}?historyLength=1",
            server.local_url, task_id
        ))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let history = body["task"]["history"]
        .as_array()
        .ok_or("should have history")?;
    assert_eq!(history.len(), 1, "historyLength=1 should return 1 entry");
    assert_eq!(
        history[0]["role"], "ROLE_USER",
        "last entry should be the user turn"
    );
    assert_eq!(history[0]["parts"][0]["text"], "turn 2");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_get_task_with_history_length_zero() -> Result<()> {
    let (server, client) = test_server().await;

    // Create a task
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"hello"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Get the task with historyLength=0 (should omit history)
    let resp = client
        .get(format!(
            "{}/tasks/{}?historyLength=0",
            server.local_url, task_id
        ))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["task"].get("history").is_none(),
        "history should be absent when historyLength=0"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_get_task_with_history_length_invalid() -> Result<()> {
    let (server, client) = test_server().await;
    let store = server.task_store();

    // Create a task
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"hello"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Append some history
    store
        .append_history(
            &task_id,
            Message {
                role: Role::Agent,
                parts: vec![Part::Text {
                    text: "response".into(),
                }],
                message_id: uuid::Uuid::new_v4().to_string(),
                extensions: None,
                metadata: None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;

    // Get with invalid historyLength (non-numeric) — should return full history
    let resp = client
        .get(format!(
            "{}/tasks/{}?historyLength=invalid",
            server.local_url, task_id
        ))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let history = body["task"]["history"]
        .as_array()
        .ok_or("should have history")?;
    assert_eq!(history.len(), 2, "full history should be returned");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_incoming_task_channel_with_context_and_parent() -> Result<()> {
    let card = AgentCard {
        name: "test".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    let mut server = A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();
    let mut task_rx = server
        .take_incoming_task_receiver()
        .ok_or("should have incoming task receiver")?;

    let client = test_client();

    // Send a task with contextId and parentTaskId
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "multi-turn"}]},
            "contextId": "ctx-999",
            "parentTaskId": "parent-888"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Verify the event was received with all fields
    let incoming = task_rx.try_recv().map_err(|e| format!("{e:?}"))?;
    assert_eq!(incoming.context_id, Some("ctx-999".into()));
    assert_eq!(incoming.parent_task_id, Some("parent-888".into()));

    server.shutdown().await;
    Ok(())
}
