use super::*;

#[tokio::test]
async fn test_tasks_send_with_context_id() {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
            "contextId": "ctx-123"
        }))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["task"]["contextId"], "ctx-123",
        "contextId should be stored"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_send_without_context_id_mints_uuid() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]}
        }))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        !body["task"]["contextId"].is_null(),
        "contextId must not be null when omitted, got: {body}"
    );
    let context_id = body["task"]["contextId"]
        .as_str()
        .ok_or("contextId should be a string when omitted")?;
    assert!(
        uuid::Uuid::parse_str(context_id).is_ok(),
        "contextId should be a UUID when omitted, got: {context_id}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_stream_without_context_id_mints_uuid() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "stream"}]}
        }))
        .send()
        .await
        .unwrap();

    // Wait for the streamed task to become visible via GET /tasks.
    let task_id = wait_for_task_in_state(
        &client,
        &server.local_url,
        "TASK_STATE_WORKING",
        Duration::from_secs(5),
    )
    .await?;

    // Cancel the task to trigger a terminal event and close the SSE stream
    client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    // Read the SSE body (stream should close after the cancel event)
    let body = tokio::time::timeout(Duration::from_secs(5), resp.bytes())
        .await
        .map_err(|_| "SSE stream should close after cancel")?
        .map_err(|e| format!("SSE body read failed: {e:?}"))?;
    let text = String::from_utf8_lossy(&body);

    let events = sse_data_events(&text);
    let task_event = events
        .iter()
        .find(|e| e.get("task").is_some())
        .ok_or("should have a task event")?;
    assert!(
        !task_event["task"]["contextId"].is_null(),
        "contextId must not be null when omitted, got: {task_event}"
    );
    let context_id = task_event["task"]["contextId"]
        .as_str()
        .ok_or("contextId should be a string when omitted")?;
    assert!(
        uuid::Uuid::parse_str(context_id).is_ok(),
        "contextId should be a UUID when omitted, got: {context_id}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_tasks_send_with_parent_task_id() {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
            "parentTaskId": "parent-456"
        }))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["task"]["parentTaskId"], "parent-456",
        "parentTaskId should be stored"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_send_with_context_and_parent() {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
            "contextId": "ctx-123",
            "parentTaskId": "parent-456"
        }))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["task"]["contextId"], "ctx-123",
        "contextId should be stored"
    );
    assert_eq!(
        body["task"]["parentTaskId"], "parent-456",
        "parentTaskId should be stored"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_send_ignores_legacy_session_field() {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
            "sessionId": "sess-ignored"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "sessionId must be ignored, not rejected"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["task"].get("sessionId").is_none(),
        "sessionId must not be echoed back on the task"
    );

    server.shutdown().await;
}
