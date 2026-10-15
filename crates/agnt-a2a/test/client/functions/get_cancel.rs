use super::*;

#[tokio::test]
async fn test_get_task() -> Result<()> {
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
    let sent = send_task(&client, &url, msg, None, None, non_blocking())
        .await
        .map_err(|e| format!("{e:?}"))?;

    let retrieved = get_task(&client, &url, &sent.id)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(retrieved.id, sent.id);
    assert_eq!(retrieved.status.state, TaskState::Working);
    Ok(())
}

#[tokio::test]
async fn test_get_task_not_found() {
    let (_server, client, url) = test_setup().await;
    let result = get_task(&client, &url, "nonexistent-id").await;
    assert!(
        matches!(result, Err(A2aError::TaskNotFound(_))),
        "Expected TaskNotFound, got: {result:?}"
    );
}

#[tokio::test]
async fn test_cancel_task() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "cancel me".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    let sent = send_task(&client, &url, msg, None, None, non_blocking())
        .await
        .map_err(|e| format!("{e:?}"))?;

    let canceled = cancel_task(&client, &url, &sent.id)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(canceled.status.state, TaskState::Canceled);
    Ok(())
}

#[tokio::test]
async fn test_cancel_already_completed() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "cancel me".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    let sent = send_task(&client, &url, msg, None, None, non_blocking())
        .await
        .map_err(|e| format!("{e:?}"))?;

    // First cancel should succeed
    let first = cancel_task(&client, &url, &sent.id)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.status.state, TaskState::Canceled);

    // Second cancel should fail — already Canceled
    let result = cancel_task(&client, &url, &sent.id).await;
    assert!(
        matches!(result, Err(A2aError::InvalidStateTransition { .. })),
        "Second cancel should return InvalidStateTransition, got: {result:?}"
    );
    Ok(())
}
