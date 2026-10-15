use super::*;

#[tokio::test]
async fn test_send_task() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let task = send_task(&client, &url, msg, None, None, non_blocking())
        .await
        .map_err(|e| format!("{e:?}"))?;

    assert!(!task.id.is_empty(), "Task should have an ID");
    assert_eq!(
        task.status.state,
        TaskState::Working,
        "New task should be in Working state"
    );
    // UUID format: 36 chars
    assert_eq!(task.id.len(), 36, "Task ID should be a UUID");
    Ok(())
}

#[tokio::test]
async fn test_send_task_with_context_id_includes_context_id_in_body() -> Result<()> {
    // -- Setup & Fixtures
    let mock = MockHttpClient::default();
    mock.expect_post_ok(
        "http://example.com/message:send",
        serde_json::json!({
            "task": {
                "id": "task-ctx",
                "contextId": "ctx-abc",
                "status": {
                    "state": "TASK_STATE_WORKING",
                    "timestamp": "2026-01-01T00:00:00Z"
                },
                "artifacts": []
            }
        }),
    );

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    // -- Exec
    let task = send_task(
        &mock,
        "http://example.com",
        msg,
        None,
        Some("ctx-abc".into()),
        None,
    )
    .await
    .map_err(|e| format!("{e:?}"))?;

    // -- Check
    let body = mock
        .last_post_body()
        .ok_or("mock should have recorded the POST body")?;
    assert_eq!(body["contextId"], "ctx-abc");
    assert_eq!(task.context_id, Some("ctx-abc".into()));
    Ok(())
}

#[tokio::test]
async fn test_send_task_without_context_id_omits_context_id_from_body() -> Result<()> {
    // -- Setup & Fixtures
    let mock = MockHttpClient::default();
    mock.expect_post_ok(
        "http://example.com/message:send",
        serde_json::json!({
            "task": {
                "id": "task-no-ctx",
                "status": {
                    "state": "TASK_STATE_WORKING",
                    "timestamp": "2026-01-01T00:00:00Z"
                },
                "artifacts": []
            }
        }),
    );

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    // -- Exec
    send_task(&mock, "http://example.com", msg, None, None, None)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
    let body = mock
        .last_post_body()
        .ok_or("mock should have recorded the POST body")?;
    assert!(
        body.get("contextId").is_none(),
        "contextId must be omitted when not provided, got: {body}"
    );
    Ok(())
}

#[tokio::test]
async fn test_send_task_with_configuration_includes_configuration_in_body() -> Result<()> {
    // -- Setup & Fixtures
    let mock = MockHttpClient::default();
    mock.expect_post_ok(
        "http://example.com/message:send",
        serde_json::json!({
            "task": {
                "id": "task-cfg",
                "status": {
                    "state": "TASK_STATE_WORKING",
                    "timestamp": "2026-01-01T00:00:00Z"
                },
                "artifacts": []
            }
        }),
    );

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    // -- Exec
    send_task(
        &mock,
        "http://example.com",
        msg,
        None,
        None,
        Some(SendMessageConfiguration {
            return_immediately: Some(true),
            accepted_output_modes: Some(vec!["text/plain".into()]),
        }),
    )
    .await
    .map_err(|e| format!("{e:?}"))?;

    // -- Check
    let body = mock
        .last_post_body()
        .ok_or("mock should have recorded the POST body")?;
    assert_eq!(body["configuration"]["returnImmediately"], true);
    assert_eq!(
        body["configuration"]["acceptedOutputModes"][0],
        "text/plain"
    );
    Ok(())
}

#[tokio::test]
async fn test_send_task_without_configuration_omits_configuration_from_body() -> Result<()> {
    // -- Setup & Fixtures
    let mock = MockHttpClient::default();
    mock.expect_post_ok(
        "http://example.com/message:send",
        serde_json::json!({
            "task": {
                "id": "task-no-cfg",
                "status": {
                    "state": "TASK_STATE_WORKING",
                    "timestamp": "2026-01-01T00:00:00Z"
                },
                "artifacts": []
            }
        }),
    );

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    // -- Exec
    send_task(&mock, "http://example.com", msg, None, None, None)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
    let body = mock
        .last_post_body()
        .ok_or("mock should have recorded the POST body")?;
    assert!(
        body.get("configuration").is_none(),
        "configuration must be omitted when not provided, got: {body}"
    );
    Ok(())
}

#[tokio::test]
async fn test_send_task_with_sender_url() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let task = send_task(
        &client,
        &url,
        msg,
        None,
        Some("http://me.local:12345".into()),
        non_blocking(),
    )
    .await
    .map_err(|e| format!("{e:?}"))?;

    assert!(!task.id.is_empty(), "Task should have an ID");
    Ok(())
}

#[tokio::test]
async fn test_send_task_connection_refused() {
    let client = A2aClient::new().unwrap();
    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let result = send_task(&client, "http://127.0.0.1:1", msg, None, None, None).await;
    assert!(
        matches!(result, Err(A2aError::ConnectionRefused(_))),
        "Expected ConnectionRefused, got: {result:?}"
    );
}
