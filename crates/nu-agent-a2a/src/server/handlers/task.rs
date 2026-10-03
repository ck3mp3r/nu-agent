use std::collections::HashMap;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde_json::{Value, json};

use crate::{A2aError, Message, TaskState};

use super::super::AppState;
use super::super::response::{a2a_error, a2a_error_with_meta, a2a_json_response, a2a_ok};

/// List tasks (spec §11.3, §11.5).
///
/// `GET /tasks` with camelCase query parameters: `status`, `pageSize`,
/// `pageToken`, `contextId`.
pub async fn handle_tasks_list(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let status = match params.get("status") {
        None => None,
        Some(s) => match TaskState::try_from(s.as_str()) {
            Ok(state) => Some(state),
            Err(e) => {
                let err = a2a_error(
                    400,
                    "INVALID_ARGUMENT",
                    "INVALID_ARGUMENT",
                    &format!("Invalid status: {e}"),
                );
                return (StatusCode::BAD_REQUEST, a2a_json_response(err));
            }
        },
    };
    let page_size = params
        .get("pageSize")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(50)
        .min(100);
    let page_token = params.get("pageToken").map(|s| s.as_str());
    let context_id = params.get("contextId").map(|s| s.as_str());

    let (tasks, next_token) = state
        .task_store
        .list_tasks_filtered(status, context_id, page_size, page_token);
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

    (StatusCode::OK, a2a_json_response(result))
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
                && let Some(history) = &mut task.history
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
                "TASK_NOT_FOUND",
                "The specified task ID does not exist or is not accessible",
                json!({"taskId": id, "timestamp": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)}),
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
            if let Err(e) = state.task_cancel_tx.send(id.clone()) {
                log::warn!("Failed to send task cancel signal for task {id}: {e}");
            }
            let result = serde_json::to_value(&task).unwrap_or_default();
            (StatusCode::OK, a2a_json_response(a2a_ok(result)))
        }
        Err(A2aError::InvalidStateTransition { from, to }) => {
            let err = a2a_error_with_meta(
                400,
                "INVALID_REQUEST",
                "TASK_NOT_CANCELABLE",
                &format!("Invalid state transition: {from:?} → {to:?}"),
                json!({"taskId": id, "from": format!("{from:?}"), "to": format!("{to:?}")}),
            );
            (StatusCode::BAD_REQUEST, a2a_json_response(err))
        }
        Err(_) => {
            let err = a2a_error_with_meta(
                404,
                "NOT_FOUND",
                "TASK_NOT_FOUND",
                "The specified task ID does not exist or is not accessible",
                json!({"taskId": id, "timestamp": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)}),
            );
            (StatusCode::NOT_FOUND, a2a_json_response(err))
        }
    }
}
