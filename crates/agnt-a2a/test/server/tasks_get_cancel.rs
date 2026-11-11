use super::*;

#[tokio::test]
async fn test_tasks_get_returns_task() -> Result<()> {
    let (server, client) = test_server().await?;

    // Create a task first
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]}}))
        .send()
        .await?;
    let send_body: serde_json::Value = send_resp.json().await?;
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Get the task
    let resp = client
        .get(format!("{}/tasks/{}", server.local_url, task_id))
        .send()
        .await?;
    let body: serde_json::Value = resp.json().await?;
    assert_eq!(body["task"]["id"], task_id);
    assert_eq!(body["task"]["status"]["state"], "TASK_STATE_WORKING");
    assert_task_timestamps_have_z_suffix(&body["task"]);

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_tasks_get_not_found() -> Result<()> {
    let (server, client) = test_server().await?;

    let resp = client
        .get(format!("{}/tasks/nonexistent-id", server.local_url))
        .send()
        .await?;
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await?;
    assert!(body.get("error").is_some());
    assert_eq!(body["error"]["code"], 404);
    assert_eq!(body["error"]["status"], "NOT_FOUND");
    assert_eq!(body["error"]["details"][0]["reason"], "TASK_NOT_FOUND");
    assert_eq!(body["error"]["details"][0]["domain"], "a2a-protocol.org");
    assert_timestamp_has_z_suffix(&body);

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_tasks_cancel_not_found() -> Result<()> {
    let (server, client) = test_server().await?;

    let resp = client
        .post(format!("{}/tasks/nonexistent-id/cancel", server.local_url))
        .send()
        .await?;
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await?;
    assert!(body.get("error").is_some());
    assert_eq!(body["error"]["code"], 404);
    assert_eq!(body["error"]["status"], "NOT_FOUND");
    assert_eq!(body["error"]["details"][0]["reason"], "TASK_NOT_FOUND");
    assert_eq!(body["error"]["details"][0]["domain"], "a2a-protocol.org");
    assert_timestamp_has_z_suffix(&body);

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_tasks_cancel() -> Result<()> {
    let (server, client) = test_server().await?;

    // Create task
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(
            &json!({"message": {"role": "user", "parts": [{"type": "text", "text": "cancel me"}]}}),
        )
        .send()
        .await?;
    let send_body: serde_json::Value = send_resp.json().await?;
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Cancel it
    let resp = client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await?;
    let body: serde_json::Value = resp.json().await?;
    assert_eq!(body["task"]["status"]["state"], "TASK_STATE_CANCELED");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_cancel_completed_fails() -> Result<()> {
    let (server, client) = test_server().await?;

    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type": "text", "text": "done"}]}}))
        .send()
        .await?;
    let send_body: serde_json::Value = send_resp.json().await?;
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // First cancel should succeed
    let _ = client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await?;

    // Second cancel should fail (already canceled)
    let resp = client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await?;
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await?;
    assert!(body.get("error").is_some(), "Second cancel should fail");
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(body["error"]["status"], "INVALID_REQUEST");
    assert_eq!(body["error"]["details"][0]["reason"], "TASK_NOT_CANCELABLE");
    assert_eq!(body["error"]["details"][0]["domain"], "a2a-protocol.org");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_task_lifecycle_full() -> Result<()> {
    let (server, client) = test_server().await?;

    // Create
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]}}))
        .send()
        .await?;
    let send_body: serde_json::Value = send_resp.json().await?;
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();
    assert_eq!(send_body["task"]["status"]["state"], "TASK_STATE_WORKING");

    // Get
    let get_resp = client
        .get(format!("{}/tasks/{}", server.local_url, task_id))
        .send()
        .await?;
    let get_body: serde_json::Value = get_resp.json().await?;
    assert_eq!(get_body["task"]["status"]["state"], "TASK_STATE_WORKING");

    // Cancel
    let cancel_resp = client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await?;
    let cancel_body: serde_json::Value = cancel_resp.json().await?;
    assert_eq!(
        cancel_body["task"]["status"]["state"],
        "TASK_STATE_CANCELED"
    );

    // Get after cancel
    let get2_resp = client
        .get(format!("{}/tasks/{}", server.local_url, task_id))
        .send()
        .await?;
    let get2_body: serde_json::Value = get2_resp.json().await?;
    assert_eq!(get2_body["task"]["status"]["state"], "TASK_STATE_CANCELED");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_concurrent_requests() -> Result<()> {
    let (server, client) = test_server().await?;
    let url = server.local_url.clone();
    let mut handles = vec![];

    for i in 0..5 {
        let c = client.clone();
        let u = url.clone();
        handles.push(tokio::spawn(async move {
            c.post(format!("{u}/message:send"))
                .json(&json!({"message": {"role": "user", "parts": [{"type": "text", "text": format!("msg-{i}")}]}}))
                .send()
                .await
                .map_err(|e| format!("{e:?}"))
        }));
    }

    for h in handles {
        let resp = h
            .await
            .map_err(|_| "worker thread panicked")?
            .map_err(|e| e.to_string())?;
        assert_eq!(resp.status(), 200);
    }

    server.shutdown().await;
    Ok(())
}
