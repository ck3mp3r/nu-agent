use std::time::Duration;

use serde_json::{Value, json};
use uuid::Uuid;

use crate::*;

/// A reqwest-based A2A client for sending JSON-RPC 2.0 requests to an A2A
/// agent and parsing typed responses.
#[derive(Clone)]
pub struct A2aClient {
    http: reqwest::Client,
    timeout: Duration,
}

impl A2aClient {
    /// Default client — 30-second timeout.
    ///
    /// The client sends `A2A-Version: 1.0` on every request per the A2A
    /// protocol (§9.2).
    pub fn new() -> Self {
        Self {
            http: default_http_client(),
            timeout: Duration::from_secs(30),
        }
    }

    /// Client with a custom timeout.
    ///
    /// The client sends `A2A-Version: 1.0` on every request per the A2A
    /// protocol (§9.2).
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
    /// Build a JSON-RPC 2.0 request envelope.
    fn build_request(&self, method: &str, params: Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": Uuid::new_v4().to_string(),
            "method": method,
            "params": params,
        })
    }

    /// Send a JSON-RPC 2.0 request via POST, parse the response, and return
    /// the `result` field (or an error).
    async fn send_jsonrpc(&self, url: &str, request: Value) -> Result<Value, A2aError> {
        let response = self
            .http
            .post(url)
            .header("Content-Type", "application/json")
            .json(&request)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| self.map_reqwest_error(e))?;

        let bytes = response
            .bytes()
            .await
            .map_err(|e| A2aError::Internal(format!("read response: {e}")))?;

        self.parse_jsonrpc_response(&bytes)
    }

    /// Parse a JSON-RPC 2.0 response from raw bytes.
    ///
    /// Extracts the `result` field on success, or maps JSON-RPC error codes
    /// to the appropriate [`A2aError`] variant.
    fn parse_jsonrpc_response(&self, bytes: &[u8]) -> Result<Value, A2aError> {
        let json: Value = serde_json::from_slice(bytes)
            .map_err(|e| A2aError::SerializationError(e.to_string()))?;
        if let Some(error) = json.get("error") {
            let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(0) as i32;
            let msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown")
                .to_string();
            return Err(match code {
                crate::TASK_NOT_FOUND => A2aError::TaskNotFound(msg),
                crate::TASK_NOT_SUPPORTED => {
                    // Parse "Invalid state transition: Working → Canceled"
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
                    code,
                    message: msg,
                    data: error.get("data").cloned(),
                },
            });
        }
        json.get("result").cloned().ok_or_else(|| {
            A2aError::SerializationError("JSON-RPC response missing result and error".into())
        })
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
    /// Posts to `{target_url}/tasks/send` with an `id`, `message`, and
    /// optionally `sessionId` at the JSON root (compatible with the A2A
    /// server's current handler).
    pub async fn send_task(
        &self,
        target_url: &str,
        message: Message,
        session_id: Option<String>,
        sender_url: Option<String>,
    ) -> Result<Task, A2aError> {
        let url = format!("{}/tasks/send", target_url.trim_end_matches('/'));

        let msg_val = serde_json::to_value(&message)?;

        let mut body = json!({
            "id": Uuid::new_v4().to_string(),
            "message": msg_val,
        });

        if let Some(sid) = session_id {
            body["sessionId"] = json!(sid);
        }
        if let Some(su) = sender_url {
            body["senderUrl"] = json!(su);
        }

        let response = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| self.map_reqwest_error(e))?;

        let bytes = response
            .bytes()
            .await
            .map_err(|e| A2aError::Internal(format!("read response: {e}")))?;

        let result = self.parse_jsonrpc_response(&bytes)?;

        serde_json::from_value(result)
            .map_err(|e| A2aError::SerializationError(format!("invalid task: {e}")))
    }

    /// Retrieve a task by ID from an A2A agent.
    ///
    /// GETs `{target_url}/tasks/{task_id}` and parses the JSON-RPC response.
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

        let result = self.parse_jsonrpc_response(&bytes)?;

        serde_json::from_value(result)
            .map_err(|e| A2aError::SerializationError(format!("invalid task: {e}")))
    }

    /// Cancel a task on an A2A agent.
    ///
    /// POSTs to `{target_url}/tasks/{task_id}/cancel` with a JSON-RPC 2.0
    /// envelope.
    pub async fn cancel_task(&self, target_url: &str, task_id: &str) -> Result<Task, A2aError> {
        let url = format!(
            "{}/tasks/{}/cancel",
            target_url.trim_end_matches('/'),
            task_id
        );

        let result = self
            .send_jsonrpc(&url, self.build_request("tasks.cancel", json!({})))
            .await?;

        serde_json::from_value(result)
            .map_err(|e| A2aError::SerializationError(format!("invalid task: {e}")))
    }

    /// List tasks from an A2A agent, optionally filtered by status.
    ///
    /// POSTs to `{target_url}/tasks/list` with an optional `status` field and
    /// parses the JSON-RPC response.
    pub async fn list_tasks(
        &self,
        target_url: &str,
        status: Option<TaskState>,
    ) -> Result<Vec<Task>, A2aError> {
        let url = format!("{}/tasks/list", target_url.trim_end_matches('/'));

        let mut body = json!({ "id": Uuid::new_v4().to_string() });
        if let Some(s) = status {
            body["status"] = json!(format!("{s}"));
        }

        let response = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| self.map_reqwest_error(e))?;

        let bytes = response
            .bytes()
            .await
            .map_err(|e| A2aError::Internal(format!("read response: {e}")))?;

        // Navigate jsonrpc -> result -> tasks
        let result = self
            .parse_jsonrpc_response(&bytes)?
            .get("tasks")
            .cloned()
            .unwrap_or_default();

        serde_json::from_value(result)
            .map_err(|e| A2aError::SerializationError(format!("invalid task list: {e}")))
    }

    /// Fetch an agent card from an A2A agent.
    ///
    /// GETs `{target_url}/agent.json` and deserializes the body directly (no
    /// JSON-RPC envelope).
    pub async fn get_agent_card(&self, target_url: &str) -> Result<AgentCard, A2aError> {
        let url = format!("{}/agent.json", target_url.trim_end_matches('/'));

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
