use std::collections::HashMap;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde_json::{Value, json};

use crate::{A2aError, Message, TaskState};

use super::super::AppState;
use super::super::response::{a2a_error_with_meta, a2a_json_response, a2a_ok};

pub async fn handle_tasks_list(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let status = body
        .get("status")
        .and_then(|v| v.as_str())
        .and_then(|s| match s {
            "TASK_STATE_SUBMITTED" | "submitted" => Some(TaskState::Submitted),
            "TASK_STATE_WORKING" | "working" => Some(TaskState::Working),
            "TASK_STATE_INPUT_REQUIRED" | "inputRequired" => Some(TaskState::InputRequired),
            "TASK_STATE_COMPLETED" | "completed" => Some(TaskState::Completed),
            "TASK_STATE_FAILED" | "failed" => Some(TaskState::Failed),
            "TASK_STATE_CANCELED" | "canceled" => Some(TaskState::Canceled),
            "TASK_STATE_REJECTED" | "rejected" => Some(TaskState::Rejected),
            _ => None,
        });
    let page_size = body
        .get("pageSize")
        .and_then(|v| v.as_u64())
        .unwrap_or(50)
        .min(100) as usize;
    let page_token = body.get("nextPageToken").and_then(|v| v.as_str());

    let (tasks, next_token) = state
        .task_store
        .list_tasks_filtered(status, page_size, page_token);
    let total_size = state.task_store.list_tasks(None).len();

    let tasks_json: Vec<Value> = tasks
        .iter()
        .filter_map(|t| serde_json::to_value(t).ok())
        .collect();

    let result = json!({
        "tasks": tasks_json,
        "totalSize": total_size,
        "pageSize": page_size,
        "nextPageToken": next_token.unwrap_or_default(),
    });

    a2a_json_response(result)
}

pub async fn handle_tasks_get(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    match state.task_store.get_task(&id) {
        Ok(mut task) => {
            // Apply historyLength filter per A2A spec §9.4.3
            let history_length = params
                .get("historyLength")
                .and_then(|v| v.parse::<i32>().ok());

            if let Some(len) = history_length
                && let Some(ref mut history) = task.history
            {
                if len == 0 {
                    task.history = None;
                } else if len > 0 {
                    let start = history.len().saturating_sub(len as usize);
                    let truncated: Vec<Message> = history.drain(start..).collect();
                    task.history = Some(truncated);
                }
                // len < 0 = return full history (no-op)
            }

            let result = serde_json::to_value(&task).unwrap_or_default();
            (StatusCode::OK, a2a_json_response(a2a_ok(result)))
        }
        Err(_) => {
            let err = a2a_error_with_meta(
                404,
                "NOT_FOUND",
                "The specified task ID does not exist or is not accessible",
                json!({"taskId": id, "timestamp": chrono::Utc::now().to_rfc3339()}),
            );
            (StatusCode::NOT_FOUND, a2a_json_response(err))
        }
    }
}

pub async fn handle_tasks_cancel(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    match state.task_store.cancel_task(&id) {
        Ok(task) => {
            let result = serde_json::to_value(&task).unwrap_or_default();
            (StatusCode::OK, a2a_json_response(a2a_ok(result)))
        }
        Err(A2aError::InvalidStateTransition { from, to }) => {
            let err = a2a_error_with_meta(
                400,
                "INVALID_REQUEST",
                &format!("Invalid state transition: {from:?} → {to:?}"),
                json!({"taskId": id, "from": format!("{from:?}"), "to": format!("{to:?}")}),
            );
            (StatusCode::BAD_REQUEST, a2a_json_response(err))
        }
        Err(_) => {
            let err = a2a_error_with_meta(
                404,
                "NOT_FOUND",
                "The specified task ID does not exist or is not accessible",
                json!({"taskId": id, "timestamp": chrono::Utc::now().to_rfc3339()}),
            );
            (StatusCode::NOT_FOUND, a2a_json_response(err))
        }
    }
}

pub async fn handle_tasks_complete(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let result_text = body.get("result").and_then(|v| v.as_str()).unwrap_or("");

    match state.task_store.complete_task(&id, result_text) {
        Ok(task) => {
            let task_json = serde_json::to_value(&task).unwrap_or_default();
            (StatusCode::OK, a2a_json_response(a2a_ok(task_json)))
        }
        Err(A2aError::InvalidStateTransition { from, to }) => {
            let err = a2a_error_with_meta(
                400,
                "INVALID_REQUEST",
                &format!("Invalid state transition: {from:?} → {to:?}"),
                json!({"taskId": id, "from": format!("{from:?}"), "to": format!("{to:?}")}),
            );
            (StatusCode::BAD_REQUEST, a2a_json_response(err))
        }
        Err(_) => {
            let err = a2a_error_with_meta(
                404,
                "NOT_FOUND",
                "The specified task ID does not exist or is not accessible",
                json!({"taskId": id, "timestamp": chrono::Utc::now().to_rfc3339()}),
            );
            (StatusCode::NOT_FOUND, a2a_json_response(err))
        }
    }
}
