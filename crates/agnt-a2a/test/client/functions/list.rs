use super::*;

#[tokio::test]
async fn test_list_tasks() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    // Send a couple of tasks
    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "first".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    send_task(&client, &url, msg, None, None, non_blocking())
        .await
        .map_err(|e| format!("{e:?}"))?;

    let msg2 = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "second".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    send_task(&client, &url, msg2, None, None, non_blocking())
        .await
        .map_err(|e| format!("{e:?}"))?;

    // List all tasks
    let tasks = list_tasks(&client, &url, None)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(tasks.len(), 2, "Should list 2 tasks");
    Ok(())
}

#[tokio::test]
async fn test_list_tasks_filtered() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    // Send a task (it starts Submitted then transitions to Working)
    let send_resp = send_task(
        &client,
        &url,
        Message {
            role: Role::User,
            parts: vec![Part::Text {
                text: "to-cancel".into(),
            }],
            message_id: uuid::Uuid::new_v4().to_string(),
            extensions: None,
            metadata: None,
        },
        None,
        None,
        non_blocking(),
    )
    .await
    .map_err(|e| format!("{e:?}"))?;

    let msg2 = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "keep".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    send_task(&client, &url, msg2, None, None, non_blocking())
        .await
        .map_err(|e| format!("{e:?}"))?;

    // Cancel the first task so it's in 'canceled' state
    cancel_task(&client, &url, &send_resp.id)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // List only working tasks
    let working = list_tasks(&client, &url, Some(TaskState::Working))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(working.len(), 1, "Should have 1 working task");
    assert_eq!(working[0].status.state, TaskState::Working);

    // List only canceled tasks
    let canceled = list_tasks(&client, &url, Some(TaskState::Canceled))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(canceled.len(), 1, "Should have 1 canceled task");
    assert_eq!(canceled[0].status.state, TaskState::Canceled);
    Ok(())
}

#[tokio::test]
async fn test_full_lifecycle() -> Result<()> {
    let (_server, client, url) = test_setup().await;

    // Send
    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "lifecycle".into(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };
    let sent = send_task(&client, &url, msg, None, None, non_blocking())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(sent.status.state, TaskState::Working);

    // Get
    let retrieved = get_task(&client, &url, &sent.id)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(retrieved.id, sent.id);

    // Cancel
    let canceled = cancel_task(&client, &url, &sent.id)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(canceled.status.state, TaskState::Canceled);

    // Get after cancel
    let final_task = get_task(&client, &url, &sent.id)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(final_task.status.state, TaskState::Canceled);
    Ok(())
}
