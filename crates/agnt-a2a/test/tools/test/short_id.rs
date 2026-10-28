//! Short task-ID resolution through the `tasks_get` and `tasks_cancel` tools.
//!
//! Each test runs against a real loopback server so the resolver's task
//! listing and the handler's client call exercise the production HTTP path.

use super::*;

#[tokio::test]
async fn test_handle_tasks_get_with_unique_prefix() -> Result<()> {
    // -- Setup & Fixtures
    let server = test_server().await;
    let ctx = tool_ctx(&server);
    let task_id = server.task_store().create_task(None, None, None).id;
    let prefix = task_id
        .get(..8)
        .ok_or("task id should be at least 8 chars")?;

    // -- Exec
    let params = serde_json::json!({"target": "test-agent", "taskId": prefix});
    let result = handle_tasks_get(ctx, params)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(
        result["taskId"], task_id,
        "response must carry the full task ID"
    );
    assert_eq!(result["state"], "TASK_STATE_SUBMITTED");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_cancel_with_unique_prefix() -> Result<()> {
    // -- Setup & Fixtures
    let server = test_server().await;
    let ctx = tool_ctx(&server);
    let task_id = server.task_store().create_task(None, None, None).id;
    let prefix = task_id
        .get(..8)
        .ok_or("task id should be at least 8 chars")?;

    // -- Exec
    let params = serde_json::json!({"target": "test-agent", "taskId": prefix});
    let result = handle_tasks_cancel(ctx, params)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(
        result["taskId"], task_id,
        "response must carry the full task ID"
    );
    assert_eq!(result["state"], "TASK_STATE_CANCELED");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_get_with_unknown_prefix() -> Result<()> {
    // -- Setup & Fixtures
    let server = test_server().await;
    let ctx = tool_ctx(&server);
    let task_id = server.task_store().create_task(None, None, None).id;
    // Flip the last hex digit of the only task's prefix: with one task in the
    // store, zero matches is guaranteed.
    let mut prefix = task_id
        .get(..8)
        .ok_or("task id should be at least 8 chars")?
        .to_string();
    let last = prefix.pop().ok_or("prefix should not be empty")?;
    let flipped = if last == '0' { '1' } else { '0' };
    prefix.push(flipped);

    // -- Exec
    let params = serde_json::json!({"target": "test-agent", "taskId": prefix});
    let err = handle_tasks_get(ctx, params)
        .await
        .err()
        .ok_or("an unknown prefix should return an error")?;

    // -- Check
    assert!(
        err.contains("no task found with id prefix"),
        "unexpected error: {err}"
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_handle_tasks_get_with_prefix_shorter_than_eight() -> Result<()> {
    // -- Setup & Fixtures
    let server = test_server().await;
    let ctx = tool_ctx(&server);
    let task_id = server.task_store().create_task(None, None, None).id;
    let too_short = task_id
        .get(..4)
        .ok_or("task id should be at least 4 chars")?;

    // -- Exec
    let params = serde_json::json!({"target": "test-agent", "taskId": too_short});
    let err = handle_tasks_get(ctx, params)
        .await
        .err()
        .ok_or("a short prefix should return an error")?;

    // -- Check
    assert!(
        err.contains("minimum of 8 characters"),
        "unexpected error: {err}"
    );

    server.shutdown().await;
    Ok(())
}
