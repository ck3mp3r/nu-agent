use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use url::Url;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Sse},
    routing::{delete, get, post},
};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::CorsLayer;

use crate::{
    A2aError, AgentCard, IncomingTask, Message, Peer, PeerCache, PushAuthenticationInfo, Role,
    TaskEvent, TaskState, TaskStore,
};

/// Shared application state injected into every axum handler.
#[derive(Clone)]
pub struct AppState {
    pub task_store: Arc<TaskStore>,
    pub agent_card: Arc<AgentCard>,
    pub incoming_tasks_tx: mpsc::Sender<IncomingTask>,
    pub peer_cache: Arc<PeerCache>,
    /// In-memory file storage for file exchange (A2A spec §6.7).
    pub files: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

/// A running A2A HTTP server with graceful shutdown.
///
/// Binds to port 0 on loopback so the OS assigns a free port. The assigned
/// port is available via `self.port` / `self.local_url` after `start()`.
pub struct A2aServer {
    pub port: u16,
    pub local_url: String,
    task_store: Arc<TaskStore>,
    shutdown_handle: Option<oneshot::Sender<()>>,
    incoming_tasks_rx: Option<mpsc::Receiver<IncomingTask>>,
}

impl A2aServer {
    /// Access the server's [`TaskStore`].
    pub fn task_store(&self) -> Arc<TaskStore> {
        self.task_store.clone()
    }
}

// ---------------------------------------------------------------------------
// A2A response helpers (spec §11.4, §11.6)
// ---------------------------------------------------------------------------

/// Wrap a task/result value in the A2A response format.
fn a2a_ok(task: Value) -> Value {
    json!({ "task": task })
}

/// Build an A2A error response body (spec §11.6).
fn a2a_error(code: u16, status: &str, message: &str) -> Value {
    json!({
        "error": {
            "code": code,
            "status": status,
            "message": message,
            "details": [{
                "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                "reason": status,
                "domain": "a2a-protocol.org",
            }],
        }
    })
}

/// Build an A2A error response with metadata.
fn a2a_error_with_meta(code: u16, status: &str, message: &str, metadata: Value) -> Value {
    json!({
        "error": {
            "code": code,
            "status": status,
            "message": message,
            "details": [{
                "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                "reason": status,
                "domain": "a2a-protocol.org",
                "metadata": metadata,
            }],
        }
    })
}

fn a2a_json_response(body: Value) -> (axum::http::HeaderMap, Json<Value>) {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        "application/a2a+json"
            .parse()
            .expect("static content-type"),
    );
    (headers, Json(body))
}

// ---------------------------------------------------------------------------
// A2A-Version middleware (A2A spec §9.2, §14.2)
// ---------------------------------------------------------------------------

/// Axum middleware that validates the incoming `A2A-Version` header on A2A API
/// paths and adds the `A2A-Version` header to every response.
async fn a2a_version_middleware(
    request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> impl IntoResponse {
    let path = request.uri().path();

    // Skip version checks for non-A2A paths (health checks, agent card discovery).
    let is_a2a_path = !matches!(
        path,
        "/health"
            | "/.well-known/agent-card.json"
            | "/extendedAgentCard"
    );

    if is_a2a_path {
        let version = request
            .headers()
            .get("A2A-Version")
            .and_then(|v| v.to_str().ok());

        match version {
            Some(v) if v == crate::A2A_VERSION => {}
            _ => {
                let error_body = a2a_error(
                    400,
                    "INVALID_REQUEST",
                    "A2A-Version header required. Supported: 1.0",
                );
                return (
                    StatusCode::BAD_REQUEST,
                    [("A2A-Version", "1.0")],
                    Json(error_body),
                )
                    .into_response();
            }
        }
    }

    let mut response = next.run(request).await;
    let _ = response.headers_mut().insert(
        "A2A-Version",
        crate::A2A_VERSION
            .parse()
            .expect("A2A_VERSION is a valid header value"),
    );
    response
}

// ---------------------------------------------------------------------------
// Route handlers
// ---------------------------------------------------------------------------

async fn handle_agent_card(State(state): State<AppState>) -> impl IntoResponse {
    let card = serde_json::to_value(&*state.agent_card).expect("serialize AgentCard");
    let version = state.agent_card.version.clone();
    let etag = format!("\"{}\"", version);
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        "application/a2a+json"
            .parse()
            .expect("static content-type"),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        "max-age=300"
            .parse()
            .expect("static CACHE_CONTROL value is always valid"),
    );
    headers.insert(
        axum::http::header::ETAG,
        etag.parse()
            .expect("card version must produce a valid ETag header value"),
    );
    (headers, Json(card))
}

async fn handle_extended_agent_card(State(state): State<AppState>) -> impl IntoResponse {
    let card = serde_json::to_value(&*state.agent_card).unwrap_or_default();
    let extended = serde_json::json!({
        "agentCard": card,
        "extendedCapabilities": {
            "streaming": true,
            "pushNotifications": false,
            "subscribeToTask": true,
            "listTasks": true,
        },
        "provider": {
            "organization": "nu-agent",
            "url": "https://github.com/ck3mp3r/nu-agent",
        },
    });
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        "application/a2a+json"
            .parse()
            .expect("static content-type"),
    );
    (headers, Json(extended))
}

async fn handle_tasks_list(State(state): State<AppState>, Json(body): Json<Value>) -> impl IntoResponse {
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

async fn handle_tasks_send(State(state): State<AppState>, Json(body): Json<Value>) -> impl IntoResponse {
    let message = match body.get("message") {
        Some(m) if m.is_object() => m,
        _ => {
            let err = a2a_error(400, "BAD_REQUEST", "Invalid request: missing 'message'");
            return (StatusCode::BAD_REQUEST, a2a_json_response(err));
        }
    };

    if message.get("role").and_then(|v| v.as_str()).is_none() {
        let err = a2a_error(400, "BAD_REQUEST", "Invalid request: message missing 'role'");
        return (StatusCode::BAD_REQUEST, a2a_json_response(err));
    }
    if message.get("parts").and_then(|v| v.as_array()).is_none() {
        let err = a2a_error(400, "BAD_REQUEST", "Invalid request: message missing 'parts'");
        return (StatusCode::BAD_REQUEST, a2a_json_response(err));
    }

    // Validate content types of each part
    if let Some(parts) = message.get("parts").and_then(|v| v.as_array()) {
        for part in parts {
            if let Some(part_type) = part.get("type").and_then(|v| v.as_str())
                && !["text", "file", "data"].contains(&part_type)
            {
                let err = a2a_error(400, "INVALID_REQUEST", &format!("Content type not supported: {part_type}"));
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
    let task = state
        .task_store
        .update_status(&task.id, TaskState::Working, None)
        .expect("Submitted → Working is a valid transition");

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
    let _ = state
        .task_store
        .append_history(&task.id, parsed_message.clone());

    // Send to event channel if a receiver is connected
    let incoming = IncomingTask {
        task_id: task.id.clone(),
        message: parsed_message,
        sender_url,
        session_id,
        context_id,
        parent_task_id,
    };
    let _ = state.incoming_tasks_tx.send(incoming).await;

    let result = serde_json::to_value(&task).unwrap_or_default();
    (StatusCode::OK, a2a_json_response(a2a_ok(result)))
}

async fn handle_tasks_get(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
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

async fn handle_tasks_cancel(
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

async fn handle_tasks_complete(
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

async fn handle_tasks_send_stream(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Sse<ReceiverStream<SseResult>>, (StatusCode, impl IntoResponse)> {
    // Validate message (same as handle_tasks_send)
    let message = match body.get("message") {
        Some(m) if m.is_object() => m,
        _ => {
            let err = a2a_error(400, "BAD_REQUEST", "Invalid request: missing 'message'");
            return Err((StatusCode::BAD_REQUEST, a2a_json_response(err)));
        }
    };

    if message.get("role").and_then(|v| v.as_str()).is_none() {
        let err = a2a_error(400, "BAD_REQUEST", "Invalid request: message missing 'role'");
        return Err((StatusCode::BAD_REQUEST, a2a_json_response(err)));
    }
    if message.get("parts").and_then(|v| v.as_array()).is_none() {
        let err = a2a_error(400, "BAD_REQUEST", "Invalid request: message missing 'parts'");
        return Err((StatusCode::BAD_REQUEST, a2a_json_response(err)));
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
    let task = state
        .task_store
        .update_status(&task.id, TaskState::Working, None)
        .expect("Submitted → Working is a valid transition");

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
    let _ = state
        .task_store
        .append_history(&task.id, parsed_message.clone());

    let incoming = IncomingTask {
        task_id: task.id.clone(),
        message: parsed_message,
        sender_url,
        session_id,
        context_id,
        parent_task_id,
    };
    let _ = state.incoming_tasks_tx.send(incoming).await;

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
                Some(TaskEvent::StatusChanged { task_id: tid, status }) => {
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
                Some(TaskEvent::ArtifactAdded { task_id: tid, artifact }) => {
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

// ---------------------------------------------------------------------------
// Subscribe (SSE) handler (spec §3.1.6)
// ---------------------------------------------------------------------------

type SseResult = Result<axum::response::sse::Event, std::convert::Infallible>;

async fn handle_tasks_subscribe(
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
                Some(TaskEvent::StatusChanged { task_id: tid, status }) => {
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
                Some(TaskEvent::ArtifactAdded { task_id: tid, artifact }) => {
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

// ---------------------------------------------------------------------------
// Push notification config handlers
// ---------------------------------------------------------------------------

async fn handle_create_push_config(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let url = body.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if url.is_empty() {
        let err = a2a_error(400, "BAD_REQUEST", "Missing required field: 'url'");
        return (StatusCode::BAD_REQUEST, a2a_json_response(err));
    }
    let auth = body
        .get("authentication")
        .and_then(|v| serde_json::from_value::<PushAuthenticationInfo>(v.clone()).ok());
    let config = state.task_store.create_push_config(&id, url, auth);
    (StatusCode::OK, a2a_json_response(json!(config)))
}

async fn handle_list_push_configs(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let configs = state.task_store.list_push_configs(&id);
    (StatusCode::OK, a2a_json_response(json!({ "configs": configs })))
}

async fn handle_delete_push_config(
    State(state): State<AppState>,
    axum::extract::Path((id, config_id)): axum::extract::Path<(String, String)>,
) -> impl IntoResponse {
    state.task_store.delete_push_config(&id, &config_id);
    (StatusCode::OK, a2a_json_response(json!({})))
}

// ---------------------------------------------------------------------------
// File exchange handlers (A2A spec §6.7)
// ---------------------------------------------------------------------------

async fn handle_file_upload(State(state): State<AppState>, body: axum::body::Bytes) -> impl IntoResponse {
    let file_id = uuid::Uuid::new_v4().to_string();
    state
        .files
        .write()
        .expect("files lock")
        .insert(file_id.clone(), body.to_vec());
    let resp = json!({
        "id": file_id,
        "url": format!("{}/files/{}", state.agent_card.url, file_id),
    });
    a2a_json_response(resp)
}

async fn handle_file_download(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let files = state.files.read().expect("files lock");
    match files.get(&id) {
        Some(data) => (StatusCode::OK, data.clone()).into_response(),
        None => (StatusCode::NOT_FOUND, "File not found").into_response(),
    }
}

// ---------------------------------------------------------------------------
// Server impl
// ---------------------------------------------------------------------------

impl A2aServer {
    /// Start the A2A server on a loopback port.
    ///
    /// The server runs in a background tokio task. Drop `self`
    /// (or call `shutdown()`) to stop it.
    ///
    /// `port` of 0 means the OS assigns a random free port.
    pub async fn start(
        agent_card: AgentCard,
        peer_cache: Arc<PeerCache>,
        port: u16,
    ) -> Result<Self, A2aError> {
        // 1. Bind to all interfaces so the server is reachable on any IP
        //    that mDNS might advertise (i.e. the host's external IP).
        let bind_addr = if port == 0 {
            "0.0.0.0:0".to_string()
        } else {
            format!("0.0.0.0:{port}")
        };
        let listener = tokio::net::TcpListener::bind(bind_addr)
            .await
            .map_err(|e| A2aError::Internal(format!("bind failed: {e}")))?;

        // 2. Read assigned port
        let port = listener
            .local_addr()
            .map_err(|e| A2aError::Internal(format!("addr failed: {e}")))?
            .port();

        // 3. Create shutdown channel
        let (tx, rx) = oneshot::channel::<()>();

        // 4. Build shared state and event channel
        let (incoming_tx, incoming_rx) = mpsc::channel::<IncomingTask>(64);
        let task_store: Arc<TaskStore> = Arc::new(TaskStore::new());
        let files: Arc<RwLock<HashMap<String, Vec<u8>>>> = Arc::new(RwLock::new(HashMap::new()));
        let state = AppState {
            task_store: task_store.clone(),
            agent_card: Arc::new(agent_card),
            incoming_tasks_tx: incoming_tx,
            peer_cache,
            files,
        };

        // 5. Build axum router with all routes (spec §11.3)
        //
        // Note: axum 0.8 requires path parameters (e.g. {id}) to be the
        // complete path segment.  Colon-suffix actions like `{id}:cancel`
        // are not supported in a single segment, so we use a separate
        // segment for the colon action when a parameter is present.
        //
        // Parameter-less colon paths (message:send, tasks:list, files:upload)
        // work fine as literal matches.
        let app = Router::new()
            .route("/health", get(|| async { "ok" }))
            // Message endpoints (§11.3.1)
            .route("/message:send", post(handle_tasks_send))
            .route("/message:stream", post(handle_tasks_send_stream))
            // Task endpoints
            .route("/tasks:list", post(handle_tasks_list))
            .route("/tasks/{id}/subscribe", get(handle_tasks_subscribe))
            .route("/tasks/{id}/cancel", post(handle_tasks_cancel))
            .route("/tasks/{id}/complete", post(handle_tasks_complete))
            .route("/tasks/{id}", get(handle_tasks_get))
            // Push notification configs (§11.3.2)
            .route(
                "/tasks/{id}/push-notifications/create",
                post(handle_create_push_config),
            )
            .route(
                "/tasks/{id}/push-notifications/list",
                get(handle_list_push_configs),
            )
            .route(
                "/tasks/{id}/push-notifications/delete/{config_id}",
                delete(handle_delete_push_config),
            )
            // File exchange
            .route("/files:upload", post(handle_file_upload))
            .route("/files/{id}", get(handle_file_download))
            // Agent card discovery (§8.6)
            .route("/.well-known/agent-card.json", get(handle_agent_card))
            .route("/extendedAgentCard", get(handle_extended_agent_card))
            .with_state(state.clone())
            .layer(middleware::from_fn(a2a_version_middleware))
            .layer(CorsLayer::permissive());

        // 6. Serve in background
        let local_url = format!("http://127.0.0.1:{port}");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    rx.await.ok();
                })
                .await
                .expect("server failed");
        });

        // 7. Return handle
        Ok(Self {
            port,
            local_url,
            task_store,
            shutdown_handle: Some(tx),
            incoming_tasks_rx: Some(incoming_rx),
        })
    }

    /// Take the incoming task event channel receiver, if any.
    ///
    /// This can be used to receive [`IncomingTask`] events when remote agents
    /// send tasks to this server. The receiver can only be taken once; returns
    /// `None` on subsequent calls.
    pub fn take_incoming_task_receiver(&mut self) -> Option<mpsc::Receiver<IncomingTask>> {
        self.incoming_tasks_rx.take()
    }

    /// Gracefully shut down the server.
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_handle.take() {
            let _ = tx.send(());
        }
    }
}
