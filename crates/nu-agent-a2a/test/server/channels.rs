use super::*;

#[tokio::test]
async fn test_incoming_task_channel() -> Result<()> {
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

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    let client = test_client();

    // Send a task with senderUrl
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&serde_json::json!({
            "message": serde_json::to_value(&msg).unwrap(),
            "senderUrl": "http://sender.local:12345"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Verify the event was received
    let incoming = task_rx.try_recv().map_err(|e| format!("{e:?}"))?;
    assert_eq!(incoming.task_id.len(), 36, "should be UUID");
    assert_eq!(incoming.sender_url, "http://sender.local:12345");

    // Verify message content
    if let Part::Text { text } = &incoming.message.parts[0] {
        assert_eq!(text, "hello");
    } else {
        panic!("expected text part");
    }

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_task_cancel_channel_emits_task_id() -> Result<()> {
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
    let mut cancel_rx = server
        .take_task_cancel_receiver()
        .ok_or("should have cancel receiver")?;
    let client = test_client();

    // Create a task
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(
            &json!({"message": {"role": "user", "parts": [{"type": "text", "text": "cancel me"}]}}),
        )
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Cancel the task
    let resp = client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["task"]["status"]["state"], "TASK_STATE_CANCELED");

    // Verify the cancel channel received the task ID
    let received = cancel_rx.try_recv().map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        received, task_id,
        "cancel channel should deliver the task ID"
    );

    server.shutdown().await;
    Ok(())
}
