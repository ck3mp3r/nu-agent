use super::*;

#[tokio::test]
async fn test_send_stream_without_accepted_output_modes_succeeds() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "stream"}]},
            "configuration": {"returnImmediately": true}
        }))
        .send()
        .await
        .unwrap();

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/event-stream"),
        "absent acceptedOutputModes must not be rejected, got: {content_type}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_stream_accepts_matching_accepted_output_modes() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "stream"}]},
            "configuration": {
                "returnImmediately": true,
                "acceptedOutputModes": ["text/plain"]
            }
        }))
        .send()
        .await
        .unwrap();

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/event-stream"),
        "matching acceptedOutputModes must be accepted, got: {content_type}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_stream_rejects_unsupported_accepted_output_modes() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "stream"}]},
            "configuration": {
                "returnImmediately": true,
                "acceptedOutputModes": ["application/json"]
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        400,
        "acceptedOutputModes with no intersection must be rejected"
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(
        body["error"]["details"][0]["reason"],
        "CONTENT_TYPE_NOT_SUPPORTED"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_stream_accepts_configuration() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "stream"}]},
            "configuration": {"returnImmediately": true}
        }))
        .send()
        .await
        .unwrap();

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/event-stream"),
        "streaming must ignore returnImmediately and stay a stream, got: {content_type}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_stream_malformed_configuration_returns_400() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "stream"}]},
            "configuration": {"returnImmediately": "yes"}
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        400,
        "malformed configuration must be rejected, not silently ignored"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(body["error"]["status"], "INVALID_ARGUMENT");
    assert_eq!(body["error"]["details"][0]["reason"], "INVALID_ARGUMENT");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_stream_returns_sse() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "stream"}]},
            "contextId": "ctx-abc"
        }))
        .send()
        .await
        .unwrap();

    // Verify SSE content type
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/event-stream"),
        "Expected SSE content type, got: {content_type}"
    );

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

    // Verify we get a task event (StreamResponse format)
    assert!(
        text.contains(r#""task""#),
        "Should have task event with StreamResponse, got: {text}"
    );

    // Verify the data contains the task in working state
    assert!(
        text.contains("TASK_STATE_WORKING"),
        "Task should be in working state, got: {text}"
    );

    // Verify we also get a cancel status update (statusUpdate format)
    assert!(
        text.contains("statusUpdate"),
        "Should have statusUpdate event for cancel, got: {text}"
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
async fn test_send_stream_invalid_body() {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:stream", server.local_url))
        .json(&json!({})) // missing message
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);

    // Read the response body and check for error
    let body = resp.bytes().await.unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains("400") || text.contains("BAD_REQUEST"),
        "Should return error for invalid body, got: {text}"
    );

    server.shutdown().await;
}
