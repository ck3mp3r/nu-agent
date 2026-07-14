use serde_json::json;

use crate::*;

use super::A2aHttpClient;

// ---------------------------------------------------------------------------
// send_task
// ---------------------------------------------------------------------------

/// Send a task to an A2A agent.
///
/// Posts to `{target_url}/message:send` (spec §11.3.1) with a `message`,
/// and optionally `sessionId` / `senderUrl`.
pub async fn send_task<C: A2aHttpClient>(
    client: &C,
    target_url: &str,
    message: Message,
    session_id: Option<String>,
    sender_url: Option<String>,
) -> Result<Task, A2aError> {
    let url = format!("{}/message:send", target_url.trim_end_matches('/'));

    let msg_val = serde_json::to_value(&message)?;

    let mut body = json!({
        "message": msg_val,
    });

    if let Some(sid) = session_id {
        body["sessionId"] = json!(sid);
    }
    if let Some(su) = sender_url {
        body["senderUrl"] = json!(su);
    }

    let result = client.post_json(&url, body).await?;

    // Parse {"task":{...}}
    let task_val = result
        .get("task")
        .cloned()
        .ok_or_else(|| A2aError::SerializationError("response missing 'task' field".into()))?;

    serde_json::from_value(task_val)
        .map_err(|e| A2aError::SerializationError(format!("invalid task: {e}")))
}

// ---------------------------------------------------------------------------
// get_task
// ---------------------------------------------------------------------------

/// Retrieve a task by ID from an A2A agent.
///
/// GETs `{target_url}/tasks/{task_id}` and parses `{"task":{...}}`.
pub async fn get_task<C: A2aHttpClient>(
    client: &C,
    target_url: &str,
    task_id: &str,
) -> Result<Task, A2aError> {
    let url = format!("{}/tasks/{}", target_url.trim_end_matches('/'), task_id);

    let bytes = client.get_bytes(&url).await?;

    let result = super::a2a_client::parse_response_body(&bytes)?;

    let task_val = result
        .get("task")
        .cloned()
        .ok_or_else(|| A2aError::SerializationError("response missing 'task' field".into()))?;

    serde_json::from_value(task_val)
        .map_err(|e| A2aError::SerializationError(format!("invalid task: {e}")))
}

// ---------------------------------------------------------------------------
// cancel_task
// ---------------------------------------------------------------------------

/// Cancel a task on an A2A agent.
///
/// POSTs to `{target_url}/tasks/{task_id}/cancel` and parses
/// `{"task":{...}}`.
pub async fn cancel_task<C: A2aHttpClient>(
    client: &C,
    target_url: &str,
    task_id: &str,
) -> Result<Task, A2aError> {
    let url = format!(
        "{}/tasks/{}/cancel",
        target_url.trim_end_matches('/'),
        task_id
    );

    let body = json!({});
    let result = client.post_json(&url, body).await?;

    let task_val = result
        .get("task")
        .cloned()
        .ok_or_else(|| A2aError::SerializationError("response missing 'task' field".into()))?;

    serde_json::from_value(task_val)
        .map_err(|e| A2aError::SerializationError(format!("invalid task: {e}")))
}

// ---------------------------------------------------------------------------
// list_tasks
// ---------------------------------------------------------------------------

/// List tasks from an A2A agent, optionally filtered by status.
///
/// POSTs to `{target_url}/tasks:list` and parses
/// `{"tasks":[...],"totalSize":N,"pageSize":N,"nextPageToken":"..."}`.
pub async fn list_tasks<C: A2aHttpClient>(
    client: &C,
    target_url: &str,
    status: Option<TaskState>,
) -> Result<Vec<Task>, A2aError> {
    let url = format!("{}/tasks:list", target_url.trim_end_matches('/'));

    let mut body = json!({});
    if let Some(s) = status {
        // Send status as string matching what server accepts
        let status_str = format!("{s}");
        body["status"] = json!(status_str);
    }

    let result = client.post_json(&url, body).await?;

    let tasks_val = result.get("tasks").cloned().unwrap_or_default();

    serde_json::from_value(tasks_val)
        .map_err(|e| A2aError::SerializationError(format!("invalid task list: {e}")))
}

// ---------------------------------------------------------------------------
// get_agent_card
// ---------------------------------------------------------------------------

/// Fetch an agent card from an A2A agent.
///
/// GETs `{target_url}/.well-known/agent-card.json` (spec §8.6) and
/// deserializes the body directly (no envelope).
pub async fn get_agent_card<C: A2aHttpClient>(
    client: &C,
    target_url: &str,
) -> Result<AgentCard, A2aError> {
    let url = format!(
        "{}/.well-known/agent-card.json",
        target_url.trim_end_matches('/')
    );

    let bytes = client.get_bytes(&url).await?;

    serde_json::from_slice(&bytes)
        .map_err(|e| A2aError::SerializationError(format!("invalid card JSON: {e}")))
}
