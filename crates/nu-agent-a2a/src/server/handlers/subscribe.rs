use std::pin::Pin;

use axum::{extract::State, http::StatusCode, response::Sse};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::{TaskEvent, task_event_to_stream_response};

use super::super::AppState;
use super::super::response::{SseError, a2a_error, a2a_error_with_meta, a2a_json_response};
use super::SseResult;

// ---------------------------------------------------------------------------
// Subscribe (SSE) handler (spec §3.1.6)
// ---------------------------------------------------------------------------

pub async fn handle_tasks_subscribe(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Sse<Pin<Box<dyn tokio_stream::Stream<Item = SseResult> + Send>>>, SseError> {
    let task = match state.task_store.get_task(&id) {
        Ok(t) => t,
        Err(_) => {
            let err = a2a_error_with_meta(
                404,
                "NOT_FOUND",
                "TASK_NOT_FOUND",
                "The specified task ID does not exist or is not accessible",
                json!({"taskId": id, "timestamp": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)}),
            );
            return Err((StatusCode::NOT_FOUND, a2a_json_response(err)).into());
        }
    };

    // Spec §3.1.6: subscribing to a task in a terminal state is an
    // UnsupportedOperationError — the stream can never produce further events.
    if task.status.state.is_terminal() {
        let err = a2a_error(
            400,
            "UNSUPPORTED_OPERATION",
            "UNSUPPORTED_OPERATION",
            "Cannot subscribe to a task in a terminal state",
        );
        return Err((StatusCode::BAD_REQUEST, a2a_json_response(err)).into());
    }

    let task_store = state.task_store.clone();
    let (mut rx, _) = task_store.subscribe(&id);
    let (tx, sse_rx) = mpsc::channel::<SseResult>(16);
    let context_id = task.context_id.clone();

    // Spawn a task that reads from the subscription channel and forwards
    // SSE events in StreamResponse format.
    tokio::spawn(async move {
        // Send initial Task as first SSE event (§3.1.6)
        let task = task_store.get_task(&id).ok();
        if let Some(t) = &task {
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

        let keepalive = tokio::time::interval(std::time::Duration::from_secs(15));
        tokio::pin!(keepalive);

        loop {
            tokio::select! {
                event = rx.recv() => {
                    match event {
                        Some(TaskEvent::StatusChanged {
                            task_id: tid,
                            status,
                        }) => {
                            let data = task_event_to_stream_response(
                                &TaskEvent::StatusChanged {
                                    task_id: tid,
                                    status: status.clone(),
                                },
                                &context_id,
                            );
                            let sent = tx
                                .send(Ok(axum::response::sse::Event::default()
                                    .data(serde_json::to_string(&data).unwrap_or_default())))
                                .await;
                            if sent.is_err() {
                                break;
                            }

                            // §11.7: resend a final Task snapshot with all
                            // artifacts before closing the stream.
                            if status.state.is_terminal() {
                                if let Ok(final_task) = task_store.get_task(&id) {
                                    let data = json!({ "task": final_task });
                                    let _ = tx
                                        .send(Ok(axum::response::sse::Event::default()
                                            .data(serde_json::to_string(&data)
                                                .unwrap_or_default())))
                                        .await;
                                }
                                break;
                            }
                        }
                        Some(TaskEvent::ArtifactAdded {
                            task_id: tid,
                            artifact,
                        }) => {
                            let data = task_event_to_stream_response(
                                &TaskEvent::ArtifactAdded {
                                    task_id: tid,
                                    artifact,
                                },
                                &context_id,
                            );
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
                _ = keepalive.tick() => {
                    if tx.send(Ok(axum::response::sse::Event::default().comment("keepalive"))).await.is_err() {
                        break;
                    }
                }
            }
        }

        log::warn!("SSE subscription handler exited for task {id}");
    });

    Ok(Sse::new(Box::pin(ReceiverStream::new(sse_rx))))
}
