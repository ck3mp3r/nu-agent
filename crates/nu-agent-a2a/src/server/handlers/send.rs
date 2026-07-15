use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde_json::Value;
use url::Url;

use crate::{IncomingTask, Message, Peer, Role, TaskState};

use super::super::AppState;
use super::super::response::{a2a_error, a2a_error_with_meta, a2a_json_response, a2a_ok};

pub async fn handle_tasks_send(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let message = match body.get("message") {
        Some(m) if m.is_object() => m,
        _ => {
            let err = a2a_error(400, "BAD_REQUEST", "Invalid request: missing 'message'");
            return (StatusCode::BAD_REQUEST, a2a_json_response(err));
        }
    };

    if message.get("role").and_then(|v| v.as_str()).is_none() {
        let err = a2a_error(
            400,
            "BAD_REQUEST",
            "Invalid request: message missing 'role'",
        );
        return (StatusCode::BAD_REQUEST, a2a_json_response(err));
    }
    if message.get("parts").and_then(|v| v.as_array()).is_none() {
        let err = a2a_error(
            400,
            "BAD_REQUEST",
            "Invalid request: message missing 'parts'",
        );
        return (StatusCode::BAD_REQUEST, a2a_json_response(err));
    }

    // Validate content types of each part
    if let Some(parts) = message.get("parts").and_then(|v| v.as_array()) {
        for part in parts {
            if let Some(part_type) = part.get("type").and_then(|v| v.as_str())
                && !["text", "file", "data"].contains(&part_type)
            {
                let err = a2a_error(
                    400,
                    "INVALID_REQUEST",
                    &format!("Content type not supported: {part_type}"),
                );
                return (StatusCode::BAD_REQUEST, a2a_json_response(err));
            }
        }
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

    let sender_url = body
        .get("senderUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Add sender to peer cache so the receiver can reply
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

    // Idempotency key support (A2A spec §3.3.1)
    let idempotency_key = body.get("idempotencyKey").and_then(|v| v.as_str());

    let task = if let Some(key) = idempotency_key {
        match state.task_store.create_task_with_idempotency(
            key,
            session_id.clone(),
            context_id.clone(),
            parent_task_id.clone(),
            metadata.clone(),
        ) {
            Ok(t) => t,
            Err(boxed) => {
                let (existing, _) = *boxed;
                let result = serde_json::to_value(&existing).unwrap_or_default();
                return (StatusCode::OK, a2a_json_response(a2a_ok(result)));
            }
        }
    } else {
        state.task_store.create_task(
            session_id.clone(),
            context_id.clone(),
            parent_task_id.clone(),
            metadata,
        )
    };
    let task_id = task.id.clone();
    let task = match state
        .task_store
        .update_status(&task.id, TaskState::Working, None)
    {
        Ok(t) => t,
        Err(e) => {
            let err = a2a_error_with_meta(
                500,
                "INTERNAL_ERROR",
                &format!("Invalid task state transition: {e}"),
                serde_json::json!({"taskId": task_id}),
            );
            return (StatusCode::INTERNAL_SERVER_ERROR, a2a_json_response(err));
        }
    };

    // Deserialize the message for the event channel, logging on failure
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

    // Send to event channel if a receiver is connected
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

    let result = serde_json::to_value(&task).unwrap_or_default();
    (StatusCode::OK, a2a_json_response(a2a_ok(result)))
}
