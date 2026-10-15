use super::*;

#[tokio::test]
async fn test_list_tasks_uses_spec_url() -> Result<()> {
    let mock = MockHttpClient::default();
    mock.expect_get_ok(
        "http://example.com/tasks",
        serde_json::to_vec(&serde_json::json!({"tasks": []}))?,
    );

    let tasks = list_tasks(&mock, "http://example.com", None)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(tasks.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_list_tasks_status_filter_uses_spec_query_param() -> Result<()> {
    let mock = MockHttpClient::default();
    mock.expect_get_ok(
        "http://example.com/tasks?status=TASK_STATE_WORKING",
        serde_json::to_vec(&serde_json::json!({"tasks": []}))?,
    );

    let tasks = list_tasks(&mock, "http://example.com", Some(TaskState::Working))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(tasks.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_cancel_task_uses_spec_url() -> Result<()> {
    let mock = MockHttpClient::default();
    mock.expect_post_ok(
        "http://example.com/tasks/task-1/cancel",
        serde_json::json!({
            "task": {
                "id": "task-1",
                "status": {
                    "state": "TASK_STATE_CANCELED",
                    "timestamp": "2026-01-01T00:00:00Z"
                },
                "artifacts": []
            }
        }),
    );

    let task = cancel_task(&mock, "http://example.com", "task-1")
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(task.status.state, TaskState::Canceled);
    Ok(())
}

#[tokio::test]
async fn test_send_task_with_trailing_slash() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    let url_with_slash = format!("{}/", url.trim_end_matches('/'));

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "trailing slash".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let task = send_task(&client, &url_with_slash, msg, None, None, non_blocking())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(!task.id.is_empty(), "Task should have an ID");
    Ok(())
}
