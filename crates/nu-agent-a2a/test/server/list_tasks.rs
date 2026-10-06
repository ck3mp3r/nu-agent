use super::*;

#[tokio::test]
async fn test_list_tasks_endpoint() -> Result<()> {
    let (server, client) = test_server().await;

    // Create a task first via send
    client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();

    // List tasks
    let resp = client
        .get(format!("{}/tasks", server.local_url))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let tasks = body["tasks"].as_array().ok_or("should have tasks array")?;
    assert_eq!(tasks.len(), 1, "Should have 1 task");
    assert!(body.get("totalSize").is_some(), "Should have totalSize");
    assert!(body.get("pageSize").is_some(), "Should have pageSize");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_list_tasks_endpoint_with_filter() -> Result<()> {
    let (server, client) = test_server().await;

    // Create a task — it starts in Submitted, then transitions to Working
    let send_resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({"message": {"role": "user", "parts": [{"type":"text","text":"hi"}]}}))
        .send()
        .await
        .unwrap();
    let send_body: serde_json::Value = send_resp.json().await.unwrap();
    let task_id = send_body["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Cancel it so it's in 'canceled' state
    client
        .post(format!("{}/tasks/{}/cancel", server.local_url, task_id))
        .send()
        .await
        .unwrap();

    // Create another task — this one stays in 'working'
    client
        .post(format!("{}/message:send", server.local_url))
        .json(
            &json!({"message": {"role": "user", "parts": [{"type":"text","text":"hello again"}]}}),
        )
        .send()
        .await
        .unwrap();

    // List with filter: working
    let resp = client
        .get(format!(
            "{}/tasks?status=TASK_STATE_WORKING",
            server.local_url
        ))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let working_tasks = body["tasks"].as_array().ok_or("should have tasks array")?;
    assert_eq!(working_tasks.len(), 1, "Should have 1 working task");
    assert_eq!(working_tasks[0]["status"]["state"], "TASK_STATE_WORKING");

    // List with filter: canceled
    let resp = client
        .get(format!(
            "{}/tasks?status=TASK_STATE_CANCELED",
            server.local_url
        ))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let canceled_tasks = body["tasks"].as_array().ok_or("should have tasks array")?;
    assert_eq!(canceled_tasks.len(), 1, "Should have 1 canceled task");
    assert_eq!(canceled_tasks[0]["status"]["state"], "TASK_STATE_CANCELED");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_list_tasks_endpoint_invalid_status_returns_400() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .get(format!("{}/tasks?status=INVALID_BOGUS", server.local_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(body["error"]["status"], "INVALID_ARGUMENT");
    let message = body["error"]["message"]
        .as_str()
        .ok_or("should have error message")?;
    assert!(
        message.contains("INVALID_BOGUS"),
        "error message should name the invalid status, got: {message}"
    );

    server.shutdown().await;
    Ok(())
}
