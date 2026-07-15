use serde_json::Value;

use super::{A2aToolContext, ToolResult};
use crate::*;

pub async fn handle(ctx: A2aToolContext, params: Value) -> ToolResult {
    let target = params
        .get("target")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'target'".to_string())?;
    let text = params
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'text'".to_string())?;
    let session_id = params
        .get("sessionId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let peer = ctx
        .cache
        .get(target)
        .ok_or_else(|| format!("Agent '{target}' not found"))?;

    let message = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: text.to_string(),
        }],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let task = send_task(
        &ctx.client,
        &peer.url,
        message,
        session_id,
        Some(ctx.own_card.url.clone()),
    )
    .await
    .map_err(|e| format!("A2A error: {e}"))?;

    // ── Background SSE watcher ──────────────────────────────────────
    let agent_name = target.to_string();
    let client = ctx.client.clone();
    let url = peer.url.clone();
    let task_id = task.id.clone();

    if let Some(completion_tx) = ctx.completion_tx.clone()
        && let Some(runtime_handle) = ctx.runtime_handle.clone()
    {
        runtime_handle.spawn(async move {
            loop {
                match client.subscribe_task(&url, &task_id).await {
                    Ok(final_task) => {
                        let result = extract_text_from_task(&final_task);
                        let event = A2aCompletionEvent {
                            task_id: final_task.id,
                            agent_name: agent_name.clone(),
                            result,
                            status: final_task.status.state,
                        };
                        if let Err(e) = completion_tx.send(event).await {
                            log::warn!("failed to send A2A completion event: {e}");
                        }
                        break;
                    }
                    Err(e) => {
                        // Check if the task completed between subscribe failures
                        match get_task(&client, &url, &task_id).await {
                            Ok(task) if task.status.state.is_terminal() => {
                                let result = extract_text_from_task(&task);
                                let event = A2aCompletionEvent {
                                    task_id: task.id,
                                    agent_name: agent_name.clone(),
                                    result,
                                    status: task.status.state,
                                };
                                let _ = completion_tx.send(event).await;
                                break;
                            }
                            Ok(_) => {
                                // Still WORKING — retry subscribe
                                log::warn!(
                                    "SSE connection dropped for task {task_id}, retrying..."
                                );
                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                continue;
                            }
                            Err(get_e) => {
                                log::warn!(
                                    "SSE watcher fail for {task_id}: {e}. get_task: {get_e}"
                                );
                                break;
                            }
                        }
                    }
                }
            }
        });
    }

    Ok(serde_json::json!({
        "taskId": task.id,
        "status": "sent",
        "message": format!("Task sent to {target}. You will be notified when it completes."),
    }))
}

/// Extract result text from a (presumably completed) task's artifacts.
///
/// Collects all [`Part::Text`] parts across all artifacts and joins them
/// with newlines.
fn extract_text_from_task(task: &Task) -> String {
    task.artifacts
        .iter()
        .flat_map(|a| a.parts.iter())
        .filter_map(|p| match p {
            Part::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
