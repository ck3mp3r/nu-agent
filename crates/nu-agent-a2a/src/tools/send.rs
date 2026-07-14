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
            match client.subscribe_task(&url, &task_id).await {
                Ok(final_task) => {
                    let result = extract_text_from_task(&final_task);
                    let event = A2aCompletionEvent {
                        task_id: final_task.id,
                        agent_name,
                        result,
                        status: final_task.status.state,
                    };
                    let _ = completion_tx.send(event).await;
                }
                Err(e) => {
                    log::warn!("A2A task watcher failed for {task_id}: {e}");
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
