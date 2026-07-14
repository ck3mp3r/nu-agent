use serde_json::Value;

use super::{A2aToolContext, ToolResult};

pub async fn handle(ctx: A2aToolContext, params: Value) -> ToolResult {
    let task_id = params
        .get("taskId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'taskId'".to_string())?;
    let result = params
        .get("result")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'result'".to_string())?;

    let store = ctx.task_store.as_ref().ok_or_else(|| {
        "No local TaskStore available (this agent cannot complete tasks)".to_string()
    })?;

    store
        .complete_task(task_id, result)
        .map(|task| serde_json::json!({"taskId": task.id, "state": task.status.state}))
        .map_err(|e| format!("A2A error: {e}"))
}
