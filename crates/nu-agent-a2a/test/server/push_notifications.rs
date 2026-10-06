use super::*;

#[tokio::test]
async fn test_subscribe_stream_receives_events() -> Result<()> {
    let (server, client) = test_server().await;

    // Create a task
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type":"text","text":"hi"}]},
            "contextId": "ctx-abc"
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Subscribe (opens SSE stream)
    let sse_resp = client
        .post(format!("{}/tasks/{}/subscribe", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    assert!(
        sse_resp.status().is_success(),
        "Subscribe should return 200"
    );

    // Verify SSE content type
    let content_type = sse_resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/event-stream"),
        "Expected SSE content type, got: {content_type}"
    );

    // Cancel the task — should trigger a status update SSE event
    client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    // Read SSE body — the stream should close after the cancel event
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let body_bytes = sse_resp.bytes().await.unwrap();
    let text = String::from_utf8_lossy(&body_bytes);

    // StreamResponse format has statusUpdate wrapper
    assert!(
        text.contains("statusUpdate"),
        "Should have statusUpdate event, got: {text}"
    );
    assert!(
        text.contains("TASK_STATE_CANCELED"),
        "Status should be canceled, got: {text}"
    );

    // Spec §4.2.1: every statusUpdate event carries the task's contextId
    let events = sse_data_events(&text);
    let status_updates: Vec<&serde_json::Value> = events
        .iter()
        .filter_map(|e| e.get("statusUpdate"))
        .collect();
    assert!(
        !status_updates.is_empty(),
        "Should have at least one statusUpdate event, got: {text}"
    );
    for update in status_updates {
        assert_eq!(
            update["contextId"], "ctx-abc",
            "statusUpdate must carry the task contextId, got: {update}"
        );
    }

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_subscribe_stream_artifact_update_carries_context_id() -> Result<()> {
    let (server, client) = test_server().await;

    // Create a task with a contextId
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type":"text","text":"hi"}]},
            "contextId": "ctx-abc"
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Subscribe (opens SSE stream)
    let sse_resp = client
        .post(format!("{}/tasks/{}/subscribe", server.local_url, task_id))
        .send()
        .await
        .unwrap();
    assert!(
        sse_resp.status().is_success(),
        "Subscribe should return 200"
    );

    // Add an artifact — should trigger an artifactUpdate SSE event
    server
        .task_store()
        .add_artifact(
            &task_id,
            Artifact {
                artifact_id: "art-1".to_string(),
                name: Some("result".to_string()),
                parts: vec![Part::Text {
                    text: "output".into(),
                }],
                metadata: None,
            },
        )
        .map_err(|e| format!("add_artifact should succeed: {e:?}"))?;

    // Cancel the task to close the SSE stream
    client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let body_bytes = sse_resp.bytes().await.unwrap();
    let text = String::from_utf8_lossy(&body_bytes);

    // Spec §4.2.2: every artifactUpdate event carries the task's contextId
    let events = sse_data_events(&text);
    let artifact_updates: Vec<&serde_json::Value> = events
        .iter()
        .filter_map(|e| e.get("artifactUpdate"))
        .collect();
    assert!(
        !artifact_updates.is_empty(),
        "Should have at least one artifactUpdate event, got: {text}"
    );
    for update in artifact_updates {
        assert_eq!(update["taskId"], task_id);
        assert_eq!(
            update["contextId"], "ctx-abc",
            "artifactUpdate must carry the task contextId, got: {update}"
        );
    }

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_subscribe_task_not_found() {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/tasks/nonexistent/subscribe", server.local_url))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("error").is_some(),
        "Should return error for nonexistent task"
    );
    assert_eq!(body["error"]["code"], 404);
    assert_eq!(body["error"]["status"], "NOT_FOUND");
    assert_eq!(body["error"]["details"][0]["reason"], "TASK_NOT_FOUND");
    assert_timestamp_has_z_suffix(&body);

    server.shutdown().await;
}

#[tokio::test]
async fn test_subscribe_terminal_task_returns_400() -> Result<()> {
    let (server, client) = test_server().await;

    // Create a task, then cancel it so it reaches a terminal state
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    // Subscribe to the terminal task — spec §3.1.6 requires an error
    let sse_resp = client
        .post(format!("{}/tasks/{}/subscribe", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    assert_eq!(
        sse_resp.status(),
        400,
        "subscribe to a terminal task must return 400"
    );

    let err_body: serde_json::Value = sse_resp.json().await.unwrap();
    assert_eq!(err_body["error"]["code"], 400);
    assert_eq!(
        err_body["error"]["details"][0]["reason"], "UNSUPPORTED_OPERATION",
        "reason must be UNSUPPORTED_OPERATION, got: {err_body}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_push_notification_payload_is_stream_response_format() -> Result<()> {
    let (server, client) = test_server().await;
    let (hook_url, mut hook_rx) = start_webhook_receiver().await?;

    // Create a task with a contextId so the payload can carry it
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type":"text","text":"hi"}]},
            "contextId": "ctx-push"
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Register the webhook
    let push_resp = client
        .post(format!(
            "{}/tasks/{}/pushNotificationConfigs",
            server.local_url, task_id
        ))
        .json(&json!({"url": hook_url}))
        .send()
        .await
        .unwrap();
    assert_eq!(push_resp.status(), 200);

    // Trigger a status change
    client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    // The webhook must receive a StreamResponse-shaped payload
    let payload = tokio::time::timeout(Duration::from_secs(5), hook_rx.recv())
        .await
        .map_err(|e| format!("timeout waiting for push notification: {e:?}"))?
        .ok_or("webhook channel closed")?;

    let update = payload
        .get("statusUpdate")
        .ok_or_else(|| format!("payload must use StreamResponse format, got: {payload}"))?;
    assert_eq!(update["taskId"], task_id);
    assert_eq!(update["contextId"], "ctx-push");
    assert_eq!(update["status"]["state"], "TASK_STATE_CANCELED");
    assert!(
        payload.get("StatusChanged").is_none(),
        "internal TaskEvent tag must not be sent to webhooks, got: {payload}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_push_config_crud_endpoints() -> Result<()> {
    let (server, client) = test_server().await;

    // Create a task first
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Create a push config
    let create_resp = client
        .post(format!(
            "{}/tasks/{}/pushNotificationConfigs",
            server.local_url, task_id
        ))
        .json(&json!({"url": "https://hook.example.com/notify"}))
        .send()
        .await
        .unwrap();
    let create_body: serde_json::Value = create_resp.json().await.unwrap();
    let config_id = create_body["id"]
        .as_str()
        .ok_or("should have config id")?
        .to_string();
    assert_eq!(create_body["url"], "https://hook.example.com/notify");

    // Get the single push config
    let get_resp = client
        .get(format!(
            "{}/tasks/{}/pushNotificationConfigs/{}",
            server.local_url, task_id, config_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(get_resp.status(), 200);
    let get_body: serde_json::Value = get_resp.json().await.unwrap();
    assert_eq!(get_body["id"], config_id);
    assert_eq!(get_body["url"], "https://hook.example.com/notify");

    // List push configs
    let list_resp = client
        .get(format!(
            "{}/tasks/{}/pushNotificationConfigs",
            server.local_url, task_id
        ))
        .send()
        .await
        .unwrap();
    let list_body: serde_json::Value = list_resp.json().await.unwrap();
    let configs = list_body["configs"]
        .as_array()
        .ok_or("should have configs array")?;
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0]["id"], config_id);

    // Delete push config
    let del_resp = client
        .delete(format!(
            "{}/tasks/{}/pushNotificationConfigs/{}",
            server.local_url, task_id, config_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(del_resp.status(), 200);

    // Verify deleted
    let list2_resp = client
        .get(format!(
            "{}/tasks/{}/pushNotificationConfigs",
            server.local_url, task_id
        ))
        .send()
        .await
        .unwrap();
    let list2_body: serde_json::Value = list2_resp.json().await.unwrap();
    let configs2 = list2_body["configs"]
        .as_array()
        .ok_or("should have configs array")?;
    assert!(configs2.is_empty(), "Push config should be deleted");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_push_config_not_found() -> Result<()> {
    let (server, client) = test_server().await;

    // Create a task
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Get a nonexistent config — spec §11.3 GET single config returns 404
    let resp = client
        .get(format!(
            "{}/tasks/{}/pushNotificationConfigs/nonexistent",
            server.local_url, task_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    // Delete is idempotent, always succeeds
    let resp = client
        .delete(format!(
            "{}/tasks/{}/pushNotificationConfigs/nonexistent",
            server.local_url, task_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_push_config_missing_url() -> Result<()> {
    let (server, client) = test_server().await;

    // Create a task
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let task_id = body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Attempt to create push config without URL
    let resp = client
        .post(format!(
            "{}/tasks/{}/pushNotificationConfigs",
            server.local_url, task_id
        ))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let resp_body: serde_json::Value = resp.json().await.unwrap();
    assert!(resp_body.get("error").is_some());
    assert_eq!(resp_body["error"]["code"], 400);
    assert_eq!(resp_body["error"]["status"], "BAD_REQUEST");
    assert_eq!(
        resp_body["error"]["details"][0]["reason"],
        "INVALID_ARGUMENT"
    );

    server.shutdown().await;
    Ok(())
}
