use super::*;

#[tokio::test]
async fn test_tasks_send_with_idempotency_key() -> Result<()> {
    let (server, client) = test_server().await;

    // First request with idempotencyKey
    let resp1 = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
            "idempotencyKey": "idem-1"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp1.status(), 200);
    let body1: serde_json::Value = resp1.json().await.unwrap();
    let task_id1 = body1["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    // Second request with same idempotencyKey should return same task
    let resp2 = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hello again"}]},
            "idempotencyKey": "idem-1"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), 200);
    let body2: serde_json::Value = resp2.json().await.unwrap();
    let task_id2 = body2["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    assert_eq!(
        task_id1, task_id2,
        "same idempotencyKey should return the same task"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_tasks_send_with_different_idempotency_keys() -> Result<()> {
    let (server, client) = test_server().await;

    let resp1 = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "first"}]},
            "idempotencyKey": "key-a"
        }))
        .send()
        .await
        .unwrap();
    let body1: serde_json::Value = resp1.json().await.unwrap();
    let task_id1 = body1["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    let resp2 = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "second"}]},
            "idempotencyKey": "key-b"
        }))
        .send()
        .await
        .unwrap();
    let body2: serde_json::Value = resp2.json().await.unwrap();
    let task_id2 = body2["task"]["id"]
        .as_str()
        .ok_or("should have task id")?
        .to_string();

    assert_ne!(
        task_id1, task_id2,
        "different idempotencyKeys should create different tasks"
    );

    server.shutdown().await;
    Ok(())
}
