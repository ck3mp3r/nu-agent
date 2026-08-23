use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Sse},
};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use url::Url;

use crate::{IncomingTask, Message, Peer, Role, TaskEvent, TaskState};

use super::super::AppState;
use super::super::response::{SseError, a2a_error, a2a_error_with_meta, a2a_json_response};
use super::SseResult;

pub async fn handle_tasks_send_stream(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Sse<ReceiverStream<SseResult>>, SseError> {
    // Validate message (same as handle_tasks_send)
    let message = match body.get("message") {
        Some(m) if m.is_object() => m,
        _ => {
            let err = a2a_error(400, "BAD_REQUEST", "Invalid request: missing 'message'");
            return Err((StatusCode::BAD_REQUEST, a2a_json_response(err)).into());
        }
    };

    if message.get("role").and_then(|v| v.as_str()).is_none() {
        let err = a2a_error(
            400,
            "BAD_REQUEST",
            "Invalid request: message missing 'role'",
        );
        return Err((StatusCode::BAD_REQUEST, a2a_json_response(err)).into());
    }
    if message.get("parts").and_then(|v| v.as_array()).is_none() {
        let err = a2a_error(
            400,
            "BAD_REQUEST",
            "Invalid request: message missing 'parts'",
        );
        return Err((StatusCode::BAD_REQUEST, a2a_json_response(err)).into());
    }

    let session_id = body
        .get("sessionId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let context_id = body
        .get("contextId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let parent_task_id = body
        .get("parentTaskId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let metadata = body
        .get("metadata")
        .filter(|v| v.is_object())
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    // Peer cache population (same as handle_tasks_send)
    let sender_url = body
        .get("senderUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if !sender_url.is_empty()
        && let Ok(parsed) = Url::parse(&sender_url)
    {
        let host = parsed.host_str().unwrap_or("unknown").to_string();
        let port = parsed.port().unwrap_or(0);
        let peer = Peer {
            name: host.clone(),
            url: sender_url.clone(),
            host,
            port,
            card: None,
            discovered_at: std::time::Instant::now(),
        };
        state.peer_cache.add_or_update(peer);
    }

    // Create task in Submitted state
    let task = state.task_store.create_task(
        session_id.clone(),
        context_id.clone(),
        parent_task_id.clone(),
        metadata,
    );

    // Subscribe BEFORE transitioning state to catch StatusChanged(Working)
    let (mut rx, _) = state.task_store.subscribe(&task.id);

    // Transition to Working (triggers notification to subscribers)
    let task_id = task.id.clone();
    let task = state
        .task_store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| {
            let err = a2a_error_with_meta(
                500,
                "INTERNAL_ERROR",
                &format!("Invalid task state transition: {e}"),
                serde_json::json!({"taskId": task_id}),
            );
            SseError::new(
                (StatusCode::INTERNAL_SERVER_ERROR, a2a_json_response(err)).into_response(),
            )
        })?;

    // Forward to incoming task event channel
    let parsed_message: Message = serde_json::from_value(
        body.get("message").cloned().unwrap_or_default(),
    )
    .unwrap_or_else(|e| {
        log::warn!("failed to deserialize incoming task message: {e}");
        Message {
            role: Role::User,
            parts: vec![],
            message_id: uuid::Uuid::new_v4().to_string(),
            extensions: None,
            metadata: None,
        }
    });

    // Store the message in the task's history for multi-turn support
    if let Err(e) = state
        .task_store
        .append_history(&task.id, parsed_message.clone())
    {
        log::warn!("failed to append task history: {e}");
    }

    let incoming = IncomingTask {
        task_id: task.id.clone(),
        message: parsed_message,
        sender_url,
        session_id,
        context_id,
        parent_task_id,
    };
    if let Err(e) = state.incoming_tasks_tx.send(incoming).await {
        log::warn!("incoming task queue full: {e}");
    }

    // Build SSE stream using StreamResponse format (spec §4.2, §11.7)
    let (tx, sse_rx) = mpsc::channel::<SseResult>(16);

    tokio::spawn(async move {
        // First event: full Task in StreamResponse format
        let initial = json!({ "task": &task });
        let _ = tx
            .send(Ok(axum::response::sse::Event::default()
                .data(serde_json::to_string(&initial).unwrap_or_default())))
            .await;

        // Forward subscription events as StreamResponse
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

                    if matches!(
                        status.state,
                        TaskState::Completed
                            | TaskState::Failed
                            | TaskState::Canceled
                            | TaskState::Rejected
                    ) {
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
