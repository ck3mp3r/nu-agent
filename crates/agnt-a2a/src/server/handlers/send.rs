use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde_json::Value;
use url::Url;

use crate::{
    IncomingTask, Message, Peer, Role, SendMessageConfiguration, Task, TaskEvent, TaskState,
    resolve_context_id,
};

use super::super::AppState;
use super::super::response::{a2a_error, a2a_error_with_meta, a2a_json_response, a2a_ok};
use super::output_modes::validate_accepted_output_modes;

/// Wait for a task to reach a terminal state, bounded by `timeout`.
///
/// Returns the task in its current state when the deadline expires before a
/// terminal state is observed (spec §3.2.2).
async fn wait_for_terminal(state: &AppState, task: Task, timeout: std::time::Duration) -> Task {
    if task.status.state.is_terminal() {
        return task;
    }

    let task_id = task.id.clone();
    let (mut rx, _) = state.task_store.subscribe(&task_id);

    // Re-read after subscribing: the task may have reached a terminal state
    // between the caller's snapshot and the subscription, in which case the
    // notification is already gone and waiting would stall until the deadline.
    if let Ok(current) = state.task_store.get_task(&task_id)
        && current.status.state.is_terminal()
    {
        return current;
    }

    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            _ = &mut deadline => break,
            event = rx.recv() => {
                match event {
                    Some(TaskEvent::StatusChanged { status, .. }) if status.state.is_terminal() => {
                        break;
                    }
                    Some(_) => continue,
                    None => break,
                }
            }
        }
    }

    state.task_store.get_task(&task_id).unwrap_or(task)
}

pub async fn handle_tasks_send(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let message = match body.get("message") {
        Some(m) if m.is_object() => m,
        _ => {
            let err = a2a_error(
                400,
                "BAD_REQUEST",
                "INVALID_ARGUMENT",
                "Invalid request: missing 'message'",
            );
            return (StatusCode::BAD_REQUEST, a2a_json_response(err));
        }
    };

    if message.get("role").and_then(|v| v.as_str()).is_none() {
        let err = a2a_error(
            400,
            "BAD_REQUEST",
            "INVALID_ARGUMENT",
            "Invalid request: message missing 'role'",
        );
        return (StatusCode::BAD_REQUEST, a2a_json_response(err));
    }
    if message.get("parts").and_then(|v| v.as_array()).is_none() {
        let err = a2a_error(
            400,
            "BAD_REQUEST",
            "INVALID_ARGUMENT",
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
                    "CONTENT_TYPE_NOT_SUPPORTED",
                    &format!("Content type not supported: {part_type}"),
                );
                return (StatusCode::BAD_REQUEST, a2a_json_response(err));
            }
        }
    }

    let context_id = Some(resolve_context_id(
        body.get("contextId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    ));

    let parent_task_id = body
        .get("parentTaskId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let metadata = body
        .get("metadata")
        .filter(|v| v.is_object())
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    // Spec §3.2.2: absent or false `returnImmediately` means blocking. A
    // present-but-malformed `configuration` is a client error, not a silent
    // fallback to blocking.
    let configuration: SendMessageConfiguration = match body.get("configuration") {
        None => SendMessageConfiguration::default(),
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(cfg) => cfg,
            Err(e) => {
                let err = a2a_error(
                    400,
                    "INVALID_ARGUMENT",
                    "INVALID_ARGUMENT",
                    &format!("Malformed 'configuration' field: {e}"),
                );
                return (StatusCode::BAD_REQUEST, a2a_json_response(err));
            }
        },
    };

    // Spec §3.2.2: reject requests whose acceptedOutputModes do not intersect
    // the modes this agent advertises in its card.
    let advertised_modes = state
        .agent_card
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .default_output_modes
        .clone();
    if let Err(message) = validate_accepted_output_modes(
        configuration.accepted_output_modes.as_deref(),
        &advertised_modes,
    ) {
        let err = a2a_error(
            400,
            "INVALID_REQUEST",
            "CONTENT_TYPE_NOT_SUPPORTED",
            &message,
        );
        return (StatusCode::BAD_REQUEST, a2a_json_response(err));
    }

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
        state
            .task_store
            .create_task(context_id.clone(), parent_task_id.clone(), metadata)
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
        context_id,
        parent_task_id,
    };
    if let Err(e) = state.incoming_tasks_tx.send(incoming).await {
        log::warn!("incoming task queue full: {e}");
    }

    // Spec §3.2.2: blocking mode waits for a terminal state before returning.
    let task = if configuration.is_return_immediately() {
        task
    } else {
        wait_for_terminal(&state, task, state.blocking_timeout).await
    };

    let result = serde_json::to_value(&task).unwrap_or_default();
    (StatusCode::OK, a2a_json_response(a2a_ok(result)))
}
