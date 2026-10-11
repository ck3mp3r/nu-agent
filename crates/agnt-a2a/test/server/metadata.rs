use super::*;

#[tokio::test]
async fn test_tasks_send_with_metadata() {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
            "metadata": {"source": "test", "priority": 5}
        }))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["task"]["metadata"]["source"], "test",
        "metadata.source should be stored"
    );
    assert_eq!(
        body["task"]["metadata"]["priority"], 5,
        "metadata.priority should be stored"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_tasks_get_returns_metadata() -> Result<()> {
    let (server, client) = test_server().await;

    // Create a task with metadata
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
            "metadata": {"source": "test", "key": "value"}
        }))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Get the task and verify metadata is present
    let resp = client
        .get(format!("{}/tasks/{task_id}", server.local_url))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["task"]["metadata"]["source"], "test",
        "metadata should be preserved when getting task"
    );
    assert_eq!(
        body["task"]["metadata"]["key"], "value",
        "metadata should be preserved when getting task"
    );

    server.shutdown().await;
    Ok(())
}
