use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Sse},
};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::{TaskEvent, TaskState};

use super::super::AppState;
use super::super::response::{a2a_error_with_meta, a2a_json_response};
use super::SseResult;

// ---------------------------------------------------------------------------
// Subscribe (SSE) handler (spec §3.1.6)
// ---------------------------------------------------------------------------

pub async fn handle_tasks_subscribe(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Sse<ReceiverStream<SseResult>>, (StatusCode, impl IntoResponse)> {
    let task = match state.task_store.get_task(&id) {
        Ok(t) => t,
        Err(_) => {
            let err = a2a_error_with_meta(
                404,
                "NOT_FOUND",
                "The specified task ID does not exist or is not accessible",
                json!({"taskId": id, "timestamp": chrono::Utc::now().to_rfc3339()}),
            );
            return Err((StatusCode::NOT_FOUND, a2a_json_response(err)));
        }
    };

    // If task is already in a terminal state, send current state and close
    let is_terminal = matches!(
        task.status.state,
        TaskState::Completed | TaskState::Failed | TaskState::Canceled | TaskState::Rejected
    );

    if is_terminal {
        let (tx, sse_rx) = mpsc::channel::<SseResult>(16);
        // Send initial task event (StreamResponse format)
        let data = json!({ "task": &task });
        let _ = tx
            .send(Ok(axum::response::sse::Event::default()
                .data(serde_json::to_string(&data).unwrap_or_default())))
            .await;
        return Ok(Sse::new(ReceiverStream::new(sse_rx)));
    }

    let task_store = state.task_store.clone();
    let (mut rx, _) = task_store.subscribe(&id);
    let (tx, sse_rx) = mpsc::channel::<SseResult>(16);

    // Spawn a task that reads from the subscription channel and forwards
    // SSE events in StreamResponse format.
    tokio::spawn(async move {
        // Send initial Task as first SSE event (§3.1.6)
        let task = task_store.get_task(&id).ok();
        if let Some(ref t) = task {
            let data = json!({ "task": t });
            if tx
                .send(Ok(axum::response::sse::Event::default()
                    .data(serde_json::to_string(&data).unwrap_or_default())))
                .await
                .is_err()
            {
                return;
            }
        }

        loop {
            match rx.recv().await {
                Some(TaskEvent::StatusChanged {
                    task_id: tid,
                    status,
                }) => {
                    let data = json!({
                        "statusUpdate": {
                            "taskId": tid,
                            "status": status,
                        }
                    });
                    let sent = tx
                        .send(Ok(axum::response::sse::Event::default()
                            .data(serde_json::to_string(&data).unwrap_or_default())))
                        .await;
                    if sent.is_err() {
                        break;
                    }

                    if status.state == TaskState::Completed
                        || status.state == TaskState::Failed
                        || status.state == TaskState::Canceled
                        || status.state == TaskState::Rejected
                    {
                        break;
                    }
                }
                Some(TaskEvent::ArtifactAdded {
                    task_id: tid,
                    artifact,
                }) => {
                    let data = json!({
                        "artifactUpdate": {
                            "taskId": tid,
                            "artifact": artifact,
                        }
                    });
                    if tx
                        .send(Ok(axum::response::sse::Event::default()
                            .data(serde_json::to_string(&data).unwrap_or_default())))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                None => break,
            }
        }
    });

    Ok(Sse::new(ReceiverStream::new(sse_rx)))
}
