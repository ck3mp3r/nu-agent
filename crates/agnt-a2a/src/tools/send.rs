use serde_json::Value;

use super::{A2aToolContext, ToolResult};
use crate::*;

#[cfg(test)]
#[path = "../../test/tools/send_context.rs"]
mod send_context_test;

pub(crate) const SEND_CONTEXT_RULE: &str = "contextId must be copied exactly and completely from a prior send result — never truncate or alter it; omit the parameter for a fresh session, and do not reuse an id across fresh sends";

/// The `contextId` a send puts on the wire, or a refusal.
///
/// An explicit id is forwarded verbatim: the caller owns the round-trip. An
/// empty or whitespace-only id is refused before any HTTP call — it names no
/// context, and carrying it would append the send to one shared empty-id
/// lineage. An omitted id stays omitted: the target boundary mints (spec
/// §3.4.1).
pub(crate) fn resolve_send_context(param: Option<&str>) -> Result<Option<String>, String> {
    match param {
        Some(id) if id.trim().is_empty() => Err(SEND_CONTEXT_RULE.to_string()),
        Some(id) => Ok(Some(id.to_string())),
        None => Ok(None),
    }
}

pub async fn handle(ctx: A2aToolContext, params: Value) -> ToolResult {
    let target = params
        .get("target")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'target'".to_string())?;
    let text = params
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'text'".to_string())?;
    let context_id = resolve_send_context(params.get("contextId").and_then(|v| v.as_str()))
        .map_err(|rule| format!("Invalid 'contextId': {rule}"))?;

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

    // Fire-and-forget: the SSE watcher below reports completion, so the
    // request must not block (spec §3.2.2).
    let configuration = SendMessageConfiguration {
        return_immediately: Some(true),
        accepted_output_modes: None,
    };

    let task = send_task(
        &ctx.client,
        &peer.url,
        message,
        Some(ctx.own_card.url.clone()),
        context_id,
        Some(configuration),
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
                        let event = A2aCompletionEvent {
                            task_id: final_task.id,
                            agent_name: agent_name.clone(),
                            status: final_task.status.state,
                            context_id: final_task.context_id,
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
                                let event = A2aCompletionEvent {
                                    task_id: task.id,
                                    agent_name: agent_name.clone(),
                                    status: task.status.state,
                                    context_id: task.context_id,
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
        "contextId": task.context_id,
        "status": "sent",
        "message": format!("Task sent to {target}. You will be notified when it completes."),
    }))
}
