use serde_json::Value;

use super::{A2aToolContext, ToolResult};
use crate::*;

pub async fn handle(ctx: A2aToolContext, params: Value) -> ToolResult {
    let status = params
        .get("status")
        .and_then(|v| v.as_str())
        .and_then(|s| match s {
            "submitted" => Some(TaskState::Submitted),
            "working" => Some(TaskState::Working),
            "inputRequired" => Some(TaskState::InputRequired),
            "completed" => Some(TaskState::Completed),
            "failed" => Some(TaskState::Failed),
            "canceled" => Some(TaskState::Canceled),
            "rejected" => Some(TaskState::Rejected),
            _ => None,
        });

    // If targeting a specific agent, delegate to the client
    if let Some(target) = params.get("target").and_then(|v| v.as_str()) {
        let peer = ctx
            .cache
            .get(target)
            .ok_or_else(|| format!("Agent '{target}' not found"))?;

        let tasks = list_tasks(&ctx.client, &peer.url, status)
            .await
            .map_err(|e| format!("A2A error: {e}"))?;

        return Ok(serde_json::json!({ "tasks": tasks }));
    }

    // No target: list from local TaskStore via A2aToolContext
    match ctx.task_store.as_ref() {
        Some(store) => {
            let (tasks, _) = store.list_tasks_filtered(status, 50, None);
            Ok(serde_json::json!({ "tasks": tasks }))
        }
        None => Ok(serde_json::json!({ "tasks": [] })),
    }
}
