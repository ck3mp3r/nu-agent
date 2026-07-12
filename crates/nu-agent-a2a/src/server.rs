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
// JSON-RPC 2.0 response helpers
// ---------------------------------------------------------------------------

fn jsonrpc_success(id: &str, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn jsonrpc_error(id: &str, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
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
    let is_a2a_path = !matches!(path, "/health" | "/agent.json" | "/agent.json/extended");

    if is_a2a_path {
        let version = request
            .headers()
            .get("A2A-Version")
            .and_then(|v| v.to_str().ok());

        match version {
            Some(v) if v == crate::A2A_VERSION => {}
            _ => {
                let error_body = jsonrpc_error(
                    "0",
                    crate::UNSUPPORTED_OPERATION,
                    "A2A-Version header required. Supported: 1.0",
                );
                return ([("A2A-Version", "1.0")], Json(error_body)).into_response();
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

async fn handle_extended_agent_card(State(state): State<AppState>) -> Json<Value> {
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
    Json(extended)
}

async fn handle_tasks_list(State(state): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let req_id = body.get("id").and_then(|v| v.as_str()).unwrap_or("0");
    let status = body
        .get("status")
        .and_then(|v| v.as_str())
        .and_then(|s| match s {
            "submitted" => Some(TaskState::Submitted),
            "working" => Some(TaskState::Working),
            "inputRequired" => Some(TaskState::InputRequired),
            "completed" => Some(TaskState::Completed),
            "failed" => Some(TaskState::Failed),
            "canceled" => Some(TaskState::Canceled),
            "rejected" => Some(TaskState::Rejected),
            _ => None,
        });
    let page_size = body
        .get("pageSize")
        .and_then(|v| v.as_u64())
        .unwrap_or(50)
        .min(100) as usize;
    let page_token = body.get("pageToken").and_then(|v| v.as_str());

    let (tasks, next_token) = state
        .task_store
        .list_tasks_filtered(status, page_size, page_token);

    let mut result = json!({
        "tasks": tasks,
    });
    if let Some(token) = next_token {
        result["nextPageToken"] = json!(token);
    }

    Json(jsonrpc_success(req_id, result))
}

async fn handle_tasks_send(State(state): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let req_id = body.get("id").and_then(|v| v.as_str()).unwrap_or("0");

    let message = match body.get("message") {
        Some(m) if m.is_object() => m,
        _ => {
            return Json(jsonrpc_error(
                req_id,
                crate::INVALID_PARAMS,
                "Invalid params: missing 'message'",
            ));
        }
    };

    if message.get("role").and_then(|v| v.as_str()).is_none() {
        return Json(jsonrpc_error(
            req_id,
            crate::INVALID_PARAMS,
            "Invalid params: message missing 'role'",
        ));
    }
    if message.get("parts").and_then(|v| v.as_array()).is_none() {
        return Json(jsonrpc_error(
            req_id,
            crate::INVALID_PARAMS,
            "Invalid params: message missing 'parts'",
        ));
    }

    // Validate content types of each part
    if let Some(parts) = message.get("parts").and_then(|v| v.as_array()) {
        for part in parts {
            if let Some(part_type) = part.get("type").and_then(|v| v.as_str())
                && !["text", "file", "data"].contains(&part_type)
            {
                return Json(jsonrpc_error(
                    req_id,
                    crate::CONTENT_TYPE_NOT_SUPPORTED,
                    &format!("Content type not supported: {part_type}"),
                ));
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
                return Json(jsonrpc_success(req_id, result));
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
    Json(jsonrpc_success(req_id, result))
}

async fn handle_tasks_get(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Json<Value> {
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
            Json(jsonrpc_success(&id, result))
        }
        Err(_) => Json(jsonrpc_error(&id, crate::TASK_NOT_FOUND, "Task not found")),
    }
}

async fn handle_tasks_cancel(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<Value> {
    match state.task_store.cancel_task(&id) {
        Ok(task) => {
            let result = serde_json::to_value(&task).unwrap_or_default();
            Json(jsonrpc_success(&id, result))
        }
        Err(A2aError::InvalidStateTransition { from, to }) => Json(jsonrpc_error(
            &id,
            crate::TASK_NOT_SUPPORTED,
            &format!("Invalid state transition: {from:?} \u{2192} {to:?}"),
        )),
        Err(_) => Json(jsonrpc_error(&id, crate::TASK_NOT_FOUND, "Task not found")),
    }
}

async fn handle_tasks_complete(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let req_id = body.get("id").and_then(|v| v.as_str()).unwrap_or("0");
    let result_text = body.get("result").and_then(|v| v.as_str()).unwrap_or("");

    match state.task_store.complete_task(&id, result_text) {
        Ok(task) => {
            let task_json = serde_json::to_value(&task).unwrap_or_default();
            Json(jsonrpc_success(req_id, task_json))
        }
        Err(A2aError::InvalidStateTransition { from, to }) => Json(jsonrpc_error(
            req_id,
            crate::TASK_NOT_SUPPORTED,
            &format!("Invalid state transition: {from:?} → {to:?}"),
        )),
        Err(_) => Json(jsonrpc_error(
            req_id,
            crate::TASK_NOT_FOUND,
            "Task not found",
        )),
    }
}

async fn handle_tasks_send_stream(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Sse<ReceiverStream<SseResult>>, (StatusCode, Json<Value>)> {
    let req_id = body.get("id").and_then(|v| v.as_str()).unwrap_or("0");

    // Validate message (same as handle_tasks_send)
    let message = match body.get("message") {
        Some(m) if m.is_object() => m,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(jsonrpc_error(
                    req_id,
                    crate::INVALID_PARAMS,
                    "Invalid params: missing 'message'",
                )),
            ));
        }
    };

    if message.get("role").and_then(|v| v.as_str()).is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(jsonrpc_error(
                req_id,
                crate::INVALID_PARAMS,
                "Invalid params: message missing 'role'",
            )),
        ));
    }
    if message.get("parts").and_then(|v| v.as_array()).is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(jsonrpc_error(
                req_id,
                crate::INVALID_PARAMS,
                "Invalid params: message missing 'parts'",
            )),
        ));
    }

    // Validate content types of each part
    if let Some(parts) = message.get("parts").and_then(|v| v.as_array()) {
        for part in parts {
            if let Some(part_type) = part.get("type").and_then(|v| v.as_str())
                && !["text", "file", "data"].contains(&part_type)
            {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(jsonrpc_error(
                        req_id,
                        crate::CONTENT_TYPE_NOT_SUPPORTED,
                        &format!("Content type not supported: {part_type}"),
                    )),
                ));
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

    // Build SSE stream: initial event + subscription forwarding
    let (tx, sse_rx) = mpsc::channel::<SseResult>(16);

    let task_json = serde_json::to_string(&task).unwrap_or_default();
    tokio::spawn(async move {
        // Send initial taskCreated event
        let _ = tx
            .send(Ok(axum::response::sse::Event::default()
                .event("taskCreated")
                .data(task_json)))
            .await;

        // Forward subscription events
        loop {
            match rx.recv().await {
                Some(TaskEvent::StatusChanged { task_id, status }) => {
                    let data = serde_json::json!({
                        "id": task_id,
                        "status": status,
                    });
                    let sent = tx
                        .send(Ok(axum::response::sse::Event::default()
                            .event("taskStatusUpdate")
                            .data(serde_json::to_string(&data).unwrap_or_default())))
                        .await;
                    if sent.is_err() {
                        break; // receiver dropped
                    }

                    // Close stream on terminal state
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
                Some(TaskEvent::ArtifactAdded { task_id, artifact }) => {
                    let data = serde_json::json!({
                        "id": task_id,
                        "artifact": artifact,
                    });
                    if tx
                        .send(Ok(axum::response::sse::Event::default()
                            .event("taskArtifactUpdate")
                            .data(serde_json::to_string(&data).unwrap_or_default())))
                        .await
                        .is_err()
                    {
                        break; // receiver dropped
                    }
                }
                None => break, // subscription channel closed
            }
        }
    });

    Ok(Sse::new(ReceiverStream::new(sse_rx)))
}

// ---------------------------------------------------------------------------
// Subscribe (SSE) handler
// ---------------------------------------------------------------------------

type SseResult = Result<axum::response::sse::Event, std::convert::Infallible>;

async fn handle_tasks_subscribe(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Sse<ReceiverStream<SseResult>>, (StatusCode, Json<Value>)> {
    let task = state.task_store.get_task(&id).map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(jsonrpc_error("0", crate::TASK_NOT_FOUND, "Task not found")),
        )
    })?;

    // If task is already in a terminal state, send current state and close
    // immediately rather than subscribing to a stream that will never produce events.
    let is_terminal = matches!(
        task.status.state,
        TaskState::Completed | TaskState::Failed | TaskState::Canceled | TaskState::Rejected
    );

    if is_terminal {
        let (tx, sse_rx) = mpsc::channel::<SseResult>(16);
        let data = serde_json::json!({
            "id": id,
            "status": task.status,
        });
        let _ = tx
            .send(Ok(axum::response::sse::Event::default()
                .event("taskStatusUpdate")
                .data(serde_json::to_string(&data).unwrap_or_default())))
            .await;
        return Ok(Sse::new(ReceiverStream::new(sse_rx)));
    }

    let task_store = state.task_store.clone();
    let (mut rx, _) = task_store.subscribe(&id);
    let (tx, sse_rx) = mpsc::channel::<SseResult>(16);

    // Spawn a task that reads from the subscription channel and forwards
    // SSE events into the response channel.
    tokio::spawn(async move {
        use axum::response::sse::Event;

        loop {
            match rx.recv().await {
                Some(TaskEvent::StatusChanged { task_id, status }) => {
                    let data = serde_json::json!({
                        "id": task_id,
                        "status": status,
                    });
                    let sent = tx
                        .send(Ok(Event::default()
                            .event("taskStatusUpdate")
                            .data(serde_json::to_string(&data).unwrap_or_default())))
                        .await;
                    if sent.is_err() {
                        break; // receiver dropped
                    }

                    // Close stream if terminal state
                    if status.state == TaskState::Completed
                        || status.state == TaskState::Failed
                        || status.state == TaskState::Canceled
                        || status.state == TaskState::Rejected
                    {
                        break;
                    }
                }
                Some(TaskEvent::ArtifactAdded { task_id, artifact }) => {
                    let data = serde_json::json!({
                        "id": task_id,
                        "artifact": artifact,
                    });
                    if tx
                        .send(Ok(Event::default()
                            .event("taskArtifactUpdate")
                            .data(serde_json::to_string(&data).unwrap_or_default())))
                        .await
                        .is_err()
                    {
                        break; // receiver dropped
                    }
                }
                None => break, // subscription channel closed
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
) -> Json<Value> {
    let req_id = body.get("id").and_then(|v| v.as_str()).unwrap_or("0");
    let url = body.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if url.is_empty() {
        return Json(jsonrpc_error(
            req_id,
            crate::INVALID_PARAMS,
            "Missing required field: 'url'",
        ));
    }
    let auth = body
        .get("authentication")
        .and_then(|v| serde_json::from_value::<PushAuthenticationInfo>(v.clone()).ok());
    let config = state.task_store.create_push_config(&id, url, auth);
    Json(jsonrpc_success(req_id, json!(config)))
}

async fn handle_list_push_configs(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<Value> {
    let configs = state.task_store.list_push_configs(&id);
    Json(jsonrpc_success("0", json!({ "configs": configs })))
}

async fn handle_get_push_config(
    State(state): State<AppState>,
    axum::extract::Path((id, config_id)): axum::extract::Path<(String, String)>,
) -> Json<Value> {
    match state.task_store.get_push_config(&id, &config_id) {
        Some(config) => Json(jsonrpc_success("0", json!(config))),
        None => Json(jsonrpc_error(
            "0",
            crate::TASK_NOT_FOUND,
            "Push notification config not found",
        )),
    }
}

async fn handle_delete_push_config(
    State(state): State<AppState>,
    axum::extract::Path((id, config_id)): axum::extract::Path<(String, String)>,
) -> Json<Value> {
    state.task_store.delete_push_config(&id, &config_id);
    Json(jsonrpc_success("0", json!({})))
}

// ---------------------------------------------------------------------------
// File exchange handlers (A2A spec §6.7)
// ---------------------------------------------------------------------------

async fn handle_file_upload(State(state): State<AppState>, body: axum::body::Bytes) -> Json<Value> {
    let file_id = uuid::Uuid::new_v4().to_string();
    state
        .files
        .write()
        .expect("files lock")
        .insert(file_id.clone(), body.to_vec());
    Json(json!({
        "id": file_id,
        "url": format!("{}/files/{}", state.agent_card.url, file_id),
    }))
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
    /// Start the A2A server on a random loopback port.
    ///
    /// The server runs in a background tokio task. Drop `self`
    /// (or call `shutdown()`) to stop it.
    pub async fn start(
        agent_card: AgentCard,
        peer_cache: Arc<PeerCache>,
    ) -> Result<Self, A2aError> {
        // 1. Bind to port 0 on 127.0.0.1
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
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

        // 5. Build axum router with all routes
        //
        // Push config and subscribe routes must be registered BEFORE {id}
        // because axum could otherwise interpret "subscribe" or
        // "push-notifications" as matching the {id} parameter.
        let app = Router::new()
            .route("/health", get(|| async { "ok" }))
            .route("/tasks/sendStream", post(handle_tasks_send_stream))
            .route("/tasks/send", post(handle_tasks_send))
            .route("/tasks/list", post(handle_tasks_list))
            .route("/tasks/{id}/subscribe", get(handle_tasks_subscribe))
            .route(
                "/tasks/{id}/push-notifications",
                post(handle_create_push_config),
            )
            .route(
                "/tasks/{id}/push-notifications",
                get(handle_list_push_configs),
            )
            .route(
                "/tasks/{id}/push-notifications/{config_id}",
                get(handle_get_push_config),
            )
            .route(
                "/tasks/{id}/push-notifications/{config_id}",
                delete(handle_delete_push_config),
            )
            .route("/tasks/{id}/complete", post(handle_tasks_complete))
            .route("/tasks/{id}", get(handle_tasks_get))
            .route("/tasks/{id}/cancel", post(handle_tasks_cancel))
            .route("/agent.json", get(handle_agent_card))
            .route("/agent.json/extended", get(handle_extended_agent_card))
            .route("/files/upload", post(handle_file_upload))
            .route("/files/{id}", get(handle_file_download))
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
