use serde_json::json;

use crate::*;

use super::A2aHttpClient;

// ---------------------------------------------------------------------------
// send_task
// ---------------------------------------------------------------------------

/// Send a task to an A2A agent.
///
/// Posts to `{target_url}/message:send` (spec §11.3.1) with a `message`,
/// and optionally `senderUrl` / `contextId` / `configuration`.
pub async fn send_task<C: A2aHttpClient>(
    client: &C,
    target_url: &str,
    message: Message,
    sender_url: Option<String>,
    context_id: Option<String>,
    configuration: Option<SendMessageConfiguration>,
) -> Result<Task, A2aError> {
    let url = format!("{}/message:send", target_url.trim_end_matches('/'));

    let msg_val = serde_json::to_value(&message)?;

    let mut body = json!({
        "message": msg_val,
    });

    if let Some(su) = sender_url {
        body["senderUrl"] = json!(su);
    }
    if let Some(cid) = context_id {
        body["contextId"] = json!(cid);
    }
    if let Some(cfg) = configuration {
        body["configuration"] = serde_json::to_value(&cfg)?;
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
/// GETs `{target_url}/tasks` (spec §11.3, §11.5) with camelCase query
/// parameters and parses
/// `{"tasks":[...],"totalSize":N,"pageSize":N,"nextPageToken":"..."}`.
pub async fn list_tasks<C: A2aHttpClient>(
    client: &C,
    target_url: &str,
    status: Option<TaskState>,
) -> Result<Vec<Task>, A2aError> {
    let mut url = format!("{}/tasks", target_url.trim_end_matches('/'));
    if let Some(s) = status {
        // Send status in the spec wire format (Display = "TASK_STATE_*")
        url.push_str(&format!("?status={s}"));
    }

    let bytes = client.get_bytes(&url).await?;
    let result = super::a2a_client::parse_response_body(&bytes)?;

    let tasks_val = result.get("tasks").cloned().unwrap_or_default();

    serde_json::from_value(tasks_val)
        .map_err(|e| A2aError::SerializationError(format!("invalid task list: {e}")))
}

// ---------------------------------------------------------------------------
// list_all_tasks
// ---------------------------------------------------------------------------

/// Maximum number of `GET /tasks` pages [`list_all_tasks`] walks.
///
/// The reference server never mints a repeating token, but a third-party
/// server that echoes a token — or mints a fresh one per page — would loop
/// forever. The cap bounds every non-termination shape.
pub const MAX_LIST_PAGES: usize = 10;

/// List every task from an A2A agent, following `nextPageToken` to exhaustion.
///
/// The server caps `pageSize` at 100 and returns a non-empty
/// `nextPageToken` until the last page, so a single `GET /tasks` can hide
/// tasks beyond the first page. This function walks all pages and returns
/// the tasks concatenated in page order.
///
/// The walk stops after [`MAX_LIST_PAGES`] pages. If the last page still
/// carries a non-empty `nextPageToken`, the tasks collected so far are
/// returned with a `warn` log; the listing is then truncated rather than
/// complete, which is recoverable, where a hang is not.
pub async fn list_all_tasks<C: A2aHttpClient>(
    client: &C,
    target_url: &str,
) -> Result<Vec<Task>, A2aError> {
    let base = target_url.trim_end_matches('/');
    let mut url = format!("{base}/tasks?pageSize=100");
    let mut all_tasks = Vec::new();

    for _ in 0..MAX_LIST_PAGES {
        let bytes = client.get_bytes(&url).await?;
        let result = super::a2a_client::parse_response_body(&bytes)?;

        let tasks_val = result.get("tasks").cloned().unwrap_or_default();
        let tasks: Vec<Task> = serde_json::from_value(tasks_val)
            .map_err(|e| A2aError::SerializationError(format!("invalid task list: {e}")))?;
        all_tasks.extend(tasks);

        let next_token = result
            .get("nextPageToken")
            .and_then(|token| token.as_str())
            .unwrap_or_default();
        if next_token.is_empty() {
            return Ok(all_tasks);
        }
        url = format!("{base}/tasks?pageSize=100&pageToken={next_token}");
    }

    log::warn!(
        "task listing from {target_url} stopped after {MAX_LIST_PAGES} pages; results may be truncated"
    );
    Ok(all_tasks)
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
