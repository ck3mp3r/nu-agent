use serde_json::Value;

use super::{A2aToolContext, ToolResult};

pub async fn handle(ctx: A2aToolContext, params: Value) -> ToolResult {
    let task_id = params
        .get("taskId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'taskId'".to_string())?;
    let target = params
        .get("target")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'target'".to_string())?;

    let peer = ctx
        .cache
        .get(target)
        .ok_or_else(|| format!("Agent '{target}' not found"))?;

    let task = crate::get_task(&ctx.client, &peer.url, task_id)
        .await
        .map_err(|e| format!("A2A error: {e}"))?;

    Ok(serde_json::json!({
        "taskId": task.id,
        "state": task.status.state,
        "artifacts": task.artifacts,
    }))
}
