use std::time::Duration;

use serde_json::{Value, json};

use crate::*;

/// A reqwest-based A2A client for sending requests to an A2A agent and
/// parsing typed responses using the A2A REST binding (spec §11).
///
/// The client sends `A2A-Version: 1.0` on every request per the A2A
/// protocol (§9.2).
#[derive(Clone)]
pub struct A2aClient {
    http: reqwest::Client,
    timeout: Duration,
}

impl A2aClient {
    /// Default client — 30-second timeout.
    pub fn new() -> Self {
        Self {
            http: default_http_client(),
            timeout: Duration::from_secs(30),
        }
    }

    /// Client with a custom timeout.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            http: default_http_client(),
            timeout,
        }
    }

    /// Full custom client (shared reqwest client, custom timeout).
    ///
    /// **Note:** unlike [`new`] and [`with_timeout`], this constructor does
    /// NOT set the `A2A-Version` default header. The caller is responsible
    /// for adding it when building their own `reqwest::Client`.
    pub fn with_client(http: reqwest::Client, timeout: Duration) -> Self {
        Self { http, timeout }
    }
}

/// Build a `reqwest::Client` with the `A2A-Version` default header set.
fn default_http_client() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::HeaderName::from_static("a2a-version"),
        reqwest::header::HeaderValue::from_static(crate::A2A_VERSION),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .expect("reqwest::Client::builder() with default headers should succeed")
}

impl Default for A2aClient {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Internal plumbing
// ---------------------------------------------------------------------------

impl A2aClient {
    /// Parse an A2A REST response from raw bytes.
    ///
    /// Success format: `{"task":{...}}` or `{"tasks":[...],...}` or direct JSON.
    /// Error format: `{"error":{"code":...,"status":"...","message":"...","details":[...]}}`
    fn parse_a2a_response(&self, bytes: &[u8]) -> Result<Value, A2aError> {
        let json: Value = serde_json::from_slice(bytes)
            .map_err(|e| A2aError::SerializationError(e.to_string()))?;

        // Check for error format first
        if let Some(error) = json.get("error") {
            let code = error.get("code").and_then(|c| c.as_u64()).unwrap_or(0) as u16;
            let msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown")
                .to_string();
            let status = error
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("UNKNOWN");

            return Err(match (code, status) {
                (404, "NOT_FOUND") => A2aError::TaskNotFound(msg),
                _ if msg.contains("Invalid state transition") => {
                    let parts: Vec<&str> = msg.split("→").collect();
                    let from_str = parts
                        .first()
                        .and_then(|s| s.split(':').next_back())
                        .unwrap_or("?")
                        .trim();
                    let to_str = parts.get(1).map(|s| s.trim()).unwrap_or("?");
                    let from = TaskState::try_from(from_str).unwrap_or(TaskState::Working);
                    let to = TaskState::try_from(to_str).unwrap_or(TaskState::Canceled);
                    A2aError::InvalidStateTransition { from, to }
                }
                _ => A2aError::JsonRpcError {
                    code: code as i32,
                    message: msg,
                    data: error.get("details").cloned(),
                },
            });
        }

        Ok(json)
    }

    /// Send a POST request and parse the response.
    async fn post_and_parse(&self, url: &str, body: Value) -> Result<Value, A2aError> {
        let body_str = serde_json::to_string(&body)
            .map_err(|e| A2aError::SerializationError(e.to_string()))?;
        let response = self
            .http
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/a2a+json")
            .body(body_str)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| self.map_reqwest_error(e))?;

        let bytes = response
            .bytes()
            .await
            .map_err(|e| A2aError::Internal(format!("read response: {e}")))?;

        self.parse_a2a_response(&bytes)
    }

    /// Map reqwest errors to typed A2aError.
    fn map_reqwest_error(&self, err: reqwest::Error) -> A2aError {
        if err.is_connect() {
            A2aError::ConnectionRefused(err.to_string())
        } else if err.is_timeout() {
            A2aError::Timeout(err.to_string())
        } else {
            A2aError::Internal(err.to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// Typed operations
// ---------------------------------------------------------------------------

impl A2aClient {
    /// Send a task to an A2A agent.
    ///
    /// Posts to `{target_url}/message:send` (spec §11.3.1) with a `message`,
    /// and optionally `sessionId` / `senderUrl`.
    pub async fn send_task(
        &self,
        target_url: &str,
        message: Message,
        session_id: Option<String>,
        sender_url: Option<String>,
    ) -> Result<Task, A2aError> {
        let url = format!("{}/message:send", target_url.trim_end_matches('/'));

        let msg_val = serde_json::to_value(&message)?;

        let mut body = json!({
            "message": msg_val,
        });

        if let Some(sid) = session_id {
            body["sessionId"] = json!(sid);
        }
        if let Some(su) = sender_url {
            body["senderUrl"] = json!(su);
        }

        let result = self.post_and_parse(&url, body).await?;

        // Parse {"task":{...}}
        let task_val = result.get("task").cloned().ok_or_else(|| {
            A2aError::SerializationError("response missing 'task' field".into())
        })?;

        serde_json::from_value(task_val)
            .map_err(|e| A2aError::SerializationError(format!("invalid task: {e}")))
    }

    /// Retrieve a task by ID from an A2A agent.
    ///
    /// GETs `{target_url}/tasks/{task_id}` and parses `{"task":{...}}`.
    pub async fn get_task(&self, target_url: &str, task_id: &str) -> Result<Task, A2aError> {
        let url = format!("{}/tasks/{}", target_url.trim_end_matches('/'), task_id);

        let response = self
            .http
            .get(&url)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| self.map_reqwest_error(e))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(A2aError::TaskNotFound(task_id.to_string()));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| A2aError::Internal(format!("read response: {e}")))?;

        let result = self.parse_a2a_response(&bytes)?;

        let task_val = result.get("task").cloned().ok_or_else(|| {
            A2aError::SerializationError("response missing 'task' field".into())
        })?;

        serde_json::from_value(task_val)
            .map_err(|e| A2aError::SerializationError(format!("invalid task: {e}")))
    }

    /// Cancel a task on an A2A agent.
    ///
    /// POSTs to `{target_url}/tasks/{task_id}/cancel` and parses
    /// `{"task":{...}}`.
    pub async fn cancel_task(&self, target_url: &str, task_id: &str) -> Result<Task, A2aError> {
        let url = format!(
            "{}/tasks/{}/cancel",
            target_url.trim_end_matches('/'),
            task_id
        );

        let body = json!({});
        let result = self.post_and_parse(&url, body).await?;

        let task_val = result.get("task").cloned().ok_or_else(|| {
            A2aError::SerializationError("response missing 'task' field".into())
        })?;

        serde_json::from_value(task_val)
            .map_err(|e| A2aError::SerializationError(format!("invalid task: {e}")))
    }

    /// List tasks from an A2A agent, optionally filtered by status.
    ///
    /// POSTs to `{target_url}/tasks:list` and parses
    /// `{"tasks":[...],"totalSize":N,"pageSize":N,"nextPageToken":"..."}`.
    pub async fn list_tasks(
        &self,
        target_url: &str,
        status: Option<TaskState>,
    ) -> Result<Vec<Task>, A2aError> {
        let url = format!("{}/tasks:list", target_url.trim_end_matches('/'));

        let mut body = json!({});
        if let Some(s) = status {
            // Send status as string matching what server accepts
            let status_str = format!("{s}");
            body["status"] = json!(status_str);
        }

        let result = self.post_and_parse(&url, body).await?;

        let tasks_val = result.get("tasks").cloned().unwrap_or_default();

        serde_json::from_value(tasks_val)
            .map_err(|e| A2aError::SerializationError(format!("invalid task list: {e}")))
    }

    /// Fetch an agent card from an A2A agent.
    ///
    /// GETs `{target_url}/.well-known/agent-card.json` (spec §8.6) and
    /// deserializes the body directly (no envelope).
    pub async fn get_agent_card(&self, target_url: &str) -> Result<AgentCard, A2aError> {
        let url = format!(
            "{}/.well-known/agent-card.json",
            target_url.trim_end_matches('/')
        );

        let response = self
            .http
            .get(&url)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| self.map_reqwest_error(e))?;

        let bytes = response
            .bytes()
            .await
            .map_err(|e| A2aError::Internal(format!("read response: {e}")))?;

        serde_json::from_slice(&bytes)
            .map_err(|e| A2aError::SerializationError(format!("invalid card JSON: {e}")))
    }

    /// Subscribe to task status changes via SSE (spec §3.1.6).
    ///
    /// GETs `{target_url}/tasks/{task_id}/subscribe` and parses the SSE stream
    /// using the StreamResponse format.
    ///
    /// Returns the final [`Task`] once a terminal state is reached, or an
    /// error if the stream closes unexpectedly.
    ///
    /// This method does NOT set a request-level timeout because the SSE
    /// connection can legitimately remain open for the lifetime of the remote
    /// task. If the underlying TCP connection drops, the stream error will
    /// be surfaced through this method.
    pub async fn subscribe_task(&self, target_url: &str, task_id: &str) -> Result<Task, A2aError> {
        let url = format!(
            "{}/tasks/{}/subscribe",
            target_url.trim_end_matches('/'),
            task_id
        );

        let mut response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| self.map_reqwest_error(e))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(A2aError::TaskNotFound(task_id.to_string()));
        }

        // ── SSE stream parsing (StreamResponse format) ────────────────────
        let mut buf = String::new();
        let mut current_data = String::new();
        let mut artifacts: Vec<Artifact> = Vec::new();
        let mut final_task_id: Option<String> = None;
        let mut context_id: Option<String> = None;

        loop {
            let chunk = response
                .chunk()
                .await
                .map_err(|e| A2aError::Internal(format!("SSE read error: {e}")))?;

            match chunk {
                Some(bytes) => {
                    if let Ok(s) = std::str::from_utf8(&bytes) {
                        buf.push_str(s);
                    }

                    // Process complete SSE events (delimited by \n\n)
                    while let Some(pos) = buf.find("\n\n") {
                        let event_str = buf[..pos].to_string();
                        buf.drain(..pos + 2);

                        // Parse data: lines
                        for line in event_str.lines() {
                            if let Some(val) = line.strip_prefix("data: ") {
                                current_data = val.to_string();
                            }
                        }

                        // Parse the StreamResponse
                        if !current_data.is_empty()
                            && let Ok(data) = serde_json::from_str::<Value>(&current_data)
                        {
                            // Initial task event: {"task":{...}}
                            if let Some(task_val) = data.get("task") {
                                if let Ok(t) =
                                    serde_json::from_value::<Task>(task_val.clone())
                                {
                                    final_task_id = Some(t.id.clone());
                                    context_id = t.context_id.clone();
                                    artifacts = t.artifacts.clone();

                                    // If terminal state, return immediately
                                    if matches!(
                                        t.status.state,
                                        TaskState::Completed
                                            | TaskState::Failed
                                            | TaskState::Canceled
                                            | TaskState::Rejected
                                    ) {
                                        return Ok(t);
                                    }
                                }
                            }
                            // Status update: {"statusUpdate":{"taskId":...,"status":{...}}}
                            else if let Some(update) = data.get("statusUpdate") {
                                let tid = update
                                    .get("taskId")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());
                                if let Some(t) = tid {
                                    final_task_id = Some(t);
                                }
                                if let Some(ctx) = update.get("contextId")
                                    && let Some(c) = ctx.as_str()
                                {
                                    context_id = Some(c.to_string());
                                }
                                if let Some(s) = update.get("status")
                                    && let Ok(ts) =
                                        serde_json::from_value::<TaskStatus>(s.clone())
                                {
                                    let is_terminal = matches!(
                                        ts.state,
                                        TaskState::Completed
                                            | TaskState::Failed
                                            | TaskState::Canceled
                                            | TaskState::Rejected
                                    );

                                    if is_terminal {
                                        let tid = final_task_id
                                            .clone()
                                            .unwrap_or_else(|| task_id.to_string());
                                        return Ok(Task {
                                            id: tid,
                                            context_id: context_id.clone(),
                                            parent_task_id: None,
                                            session_id: None,
                                            status: ts,
                                            history: None,
                                            artifacts: std::mem::take(&mut artifacts),
                                            created_at: None,
                                            metadata: None,
                                        });
                                    }
                                }
                            }
                            // Artifact update: {"artifactUpdate":{"taskId":...,"artifact":{...}}}
                            else if let Some(update) = data.get("artifactUpdate")
                                && let Some(artifact_val) = update.get("artifact")
                                && let Ok(a) =
                                    serde_json::from_value::<Artifact>(
                                        artifact_val.clone(),
                                    )
                                && !artifacts
                                    .iter()
                                    .any(|ea| ea.artifact_id == a.artifact_id)
                            {
                                artifacts.push(a);
                            }
                        }

                        current_data.clear();
                    }
                }
                None => {
                    return Err(A2aError::Internal(
                        "SSE stream closed without terminal state".into(),
                    ));
                }
            }
        }
    }

    /// List all known peers from the peer cache.
    ///
    /// Synchronous — no HTTP call. Reads directly from the in-memory cache.
    pub fn list_peers(&self, cache: &PeerCache) -> Vec<Peer> {
        cache.list()
    }

    /// Get a single peer by name from the peer cache.
    ///
    /// Synchronous — no HTTP call.
    pub fn get_peer(&self, cache: &PeerCache, name: &str) -> Option<Peer> {
        cache.get(name)
    }
}
