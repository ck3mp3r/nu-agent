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

    // Give the server a moment to create the task
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Look up the task via list to get its ID
    let list_resp = client
        .get(format!("{}/tasks", server.local_url))
        .send()
        .await
        .unwrap();
    let list_body: serde_json::Value = list_resp.json().await.unwrap();
    let tasks = list_body["tasks"]
        .as_array()
        .ok_or("should have tasks array")?;
    let task_id = tasks
        .iter()
        .find(|t| t["status"]["state"] == "TASK_STATE_WORKING")
        .and_then(|t| t["id"].as_str())
        .ok_or("should find a working task")?;

    // Cancel the task to trigger a terminal event and close the SSE stream
    client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    // Read the SSE body (stream should close after the cancel event)
    tokio::time::sleep(Duration::from_millis(200)).await;
    let body = resp.bytes().await.unwrap();
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
