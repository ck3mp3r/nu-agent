use super::*;

#[tokio::test]
async fn test_tasks_send_creates_task() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {
                "role": "user",
                "parts": [{"type": "text", "text": "hello"}]
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("task").is_some(), "Should have a task field");
    let task_id = body["task"]["id"].as_str().ok_or("should have task id")?;
    assert!(!task_id.is_empty(), "Task should have an ID");
    assert_eq!(body["task"]["status"]["state"], "TASK_STATE_WORKING");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_with_return_immediately_returns_working_task() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
            "configuration": {"returnImmediately": true}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["task"]["status"]["state"], "TASK_STATE_WORKING",
        "non-blocking send must return the in-progress task, got: {body}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_without_configuration_blocks_until_terminal() -> Result<()> {
    let (server, client) = test_server_with_blocking_timeout(MAX_TEST_BLOCKING_TIMEOUT).await;

    // Cancel the task from another connection once the blocking send has
    // created it.
    let store = server.task_store();
    tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let mut ticker = tokio::time::interval(Duration::from_millis(10));
        loop {
            ticker.tick().await;
            if let Some(task) = store.list_tasks(None).first() {
                let _ = store.update_status(&task.id, TaskState::Canceled, None);
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
        }
    });

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "block me"}]}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["task"]["status"]["state"], "TASK_STATE_CANCELED",
        "blocking send must return the terminal task, got: {body}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_blocking_timeout_returns_non_terminal_task() -> Result<()> {
    let (server, client) = test_server_with_blocking_timeout(Duration::from_millis(200)).await;

    let started = std::time::Instant::now();
    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "never finishes"}]},
            "configuration": {"returnImmediately": false}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(
        started.elapsed() >= Duration::from_millis(200),
        "blocking send must wait for the deadline, elapsed: {:?}",
        started.elapsed()
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["task"]["status"]["state"], "TASK_STATE_WORKING",
        "timed-out blocking send must return the non-terminal task, got: {body}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_with_accepted_output_modes_parses_without_error() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
            "configuration": {
                "returnImmediately": true,
                "acceptedOutputModes": ["text/plain", "application/json"]
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("task").is_some(),
        "acceptedOutputModes must parse without error, got: {body}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_without_accepted_output_modes_succeeds() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
            "configuration": {"returnImmediately": true}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("task").is_some(),
        "absent acceptedOutputModes must not be rejected, got: {body}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_rejects_unsupported_accepted_output_modes() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
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
async fn test_send_accepts_card_advertised_output_mode() -> Result<()> {
    let (server, client) =
        test_server_with_output_modes(vec!["application/json".to_string()]).await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
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
        200,
        "a mode advertised by the card must be accepted"
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("task").is_some(),
        "should create a task, got: {body}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_rejects_mode_not_advertised_by_card() -> Result<()> {
    let (server, client) =
        test_server_with_output_modes(vec!["application/json".to_string()]).await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
            "configuration": {
                "returnImmediately": true,
                "acceptedOutputModes": ["text/plain"]
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        400,
        "a mode the card does not advertise must be rejected"
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["error"]["details"][0]["reason"],
        "CONTENT_TYPE_NOT_SUPPORTED"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_send_malformed_configuration_returns_400() -> Result<()> {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({
            "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
            "configuration": {"returnImmediately": "yes"}
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        400,
        "malformed configuration must be rejected, not silently defaulted"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(body["error"]["status"], "INVALID_ARGUMENT");
    assert_eq!(body["error"]["details"][0]["reason"], "INVALID_ARGUMENT");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_tasks_send_missing_message() {
    let (server, client) = test_server().await;

    let resp = client
        .post(format!("{}/message:send", server.local_url))
        .json(&json!({}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("error").is_some(),
        "Should return error for missing message"
    );
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(body["error"]["status"], "BAD_REQUEST");
    assert_eq!(body["error"]["details"][0]["reason"], "INVALID_ARGUMENT");
    assert_eq!(body["error"]["details"][0]["domain"], "a2a-protocol.org");

    server.shutdown().await;
}
