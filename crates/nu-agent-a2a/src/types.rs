use std::collections::HashMap;
use std::fmt;

use chrono::{DateTime, Utc};
use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

// ---------------------------------------------------------------------------
// A2A protocol version (A2A spec §9.2, §14.2)
// ---------------------------------------------------------------------------

/// A2A protocol version sent as the `A2A-Version` HTTP header on every
/// request and response.
pub const A2A_VERSION: &str = "1.0";

// ---------------------------------------------------------------------------
// TaskState (A2A spec §9.6)
// ---------------------------------------------------------------------------

/// The state of an A2A task.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskState {
    #[default]
    Unspecified,
    Submitted,
    Working,
    InputRequired,
    Completed,
    Failed,
    Canceled,
    Rejected,
    AuthRequired,
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TaskState::Unspecified => "TASK_STATE_UNSPECIFIED",
            TaskState::Submitted => "TASK_STATE_SUBMITTED",
            TaskState::Working => "TASK_STATE_WORKING",
            TaskState::InputRequired => "TASK_STATE_INPUT_REQUIRED",
            TaskState::Completed => "TASK_STATE_COMPLETED",
            TaskState::Failed => "TASK_STATE_FAILED",
            TaskState::Canceled => "TASK_STATE_CANCELED",
            TaskState::Rejected => "TASK_STATE_REJECTED",
            TaskState::AuthRequired => "TASK_STATE_AUTH_REQUIRED",
        };
        f.write_str(s)
    }
}

impl TryFrom<&str> for TaskState {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            // SCREAMING_SNAKE_CASE (spec format)
            "TASK_STATE_UNSPECIFIED" => Ok(TaskState::Unspecified),
            "TASK_STATE_SUBMITTED" => Ok(TaskState::Submitted),
            "TASK_STATE_WORKING" => Ok(TaskState::Working),
            "TASK_STATE_INPUT_REQUIRED" => Ok(TaskState::InputRequired),
            "TASK_STATE_COMPLETED" => Ok(TaskState::Completed),
            "TASK_STATE_FAILED" => Ok(TaskState::Failed),
            "TASK_STATE_CANCELED" => Ok(TaskState::Canceled),
            "TASK_STATE_REJECTED" => Ok(TaskState::Rejected),
            "TASK_STATE_AUTH_REQUIRED" => Ok(TaskState::AuthRequired),
            // Legacy lowercase strings (backward compat)
            "submitted" => Ok(TaskState::Submitted),
            "working" => Ok(TaskState::Working),
            "inputRequired" => Ok(TaskState::InputRequired),
            "completed" => Ok(TaskState::Completed),
            "failed" => Ok(TaskState::Failed),
            "canceled" => Ok(TaskState::Canceled),
            "rejected" => Ok(TaskState::Rejected),
            _ => Err(format!("unknown task state: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// TaskStatus (A2A spec §9.3)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskStatus {
    pub state: TaskState,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
}

// ---------------------------------------------------------------------------
// Role (A2A spec §4.6)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Role {
    User,
    Agent,
}

// ---------------------------------------------------------------------------
// Part content types — wrapper structs for the untagged file/data variants
// ---------------------------------------------------------------------------

/// Content of a file part (A2A spec §6.7).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileContent {
    pub url: String,
    pub filename: String,
    #[serde(rename = "mediaType")]
    pub media_type: String,
}

/// Content of a data part (A2A spec §6.7).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DataContent {
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub schema: Value,
}

// ---------------------------------------------------------------------------
// Part (A2A spec §6.7)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Part {
    Text {
        text: String,
    },
    File {
        file: FileContent,
    },
    Data {
        data: DataContent,
    },
}

// ---------------------------------------------------------------------------
// Message (A2A spec §4.6)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
    #[serde(rename = "messageId")]
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
}

// ---------------------------------------------------------------------------
// IncomingTask — event channel type for received tasks
// ---------------------------------------------------------------------------

/// An incoming task received from a remote A2A agent via the HTTP API.
///
/// This is delivered to consumers via the event channel obtained from
/// [`A2aServer::take_incoming_task_receiver`].
#[derive(Clone, Debug)]
pub struct IncomingTask {
    /// The server-assigned UUID of the task.
    pub task_id: String,
    /// The deserialised user message.
    pub message: Message,
    /// The `senderUrl` sent by the remote agent (may be empty).
    pub sender_url: String,
    /// Optional session identifier.
    pub session_id: Option<String>,
    /// Optional multi-turn conversation context identifier.
    pub context_id: Option<String>,
    /// Optional identifier of the parent task that spawned this one.
    pub parent_task_id: Option<String>,
}

// ---------------------------------------------------------------------------
// A2aCompletionEvent
// ---------------------------------------------------------------------------

/// A completion event delivered when a remote agent finishes processing
/// a task that was sent via `tasks.send`.
///
/// This is produced by a background SSE watcher and delivered to the agent
/// runtime via a shared channel, so the LLM sees a completion message on
/// the next turn without having to poll.
#[derive(Clone, Debug)]
pub struct A2aCompletionEvent {
    pub task_id: String,
    pub agent_name: String,
    /// Concatenated text parts from the final task result text (from
    /// artifacts or status message).
    pub result: String,
    pub status: TaskState,
}

// ---------------------------------------------------------------------------
// Artifact (A2A spec §9.3)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    #[serde(rename = "artifactId")]
    pub artifact_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub parts: Vec<Part>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
}

// ---------------------------------------------------------------------------
// TaskEvent — notification channel event for live subscribers
// ---------------------------------------------------------------------------

/// An event sent to subscribed SSE clients when a task's status changes or
/// an artifact is added.
#[derive(Clone, Debug, Serialize)]
pub enum TaskEvent {
    StatusChanged { task_id: String, status: TaskStatus },
    ArtifactAdded { task_id: String, artifact: Artifact },
}

// ---------------------------------------------------------------------------
// Push notification configs
// ---------------------------------------------------------------------------

/// A webhook push notification configuration for a task.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PushNotificationConfig {
    pub id: String,
    pub url: String,
    #[serde(rename = "taskId")]
    pub task_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<PushAuthenticationInfo>,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
}

/// Authentication information for a push notification webhook.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PushAuthenticationInfo {
    #[serde(flatten)]
    pub scheme: PushAuthScheme,
}

/// Authentication scheme for push notification webhooks.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "scheme")]
pub enum PushAuthScheme {
    Bearer {
        token: String,
    },
    Basic {
        username: String,
        password: String,
    },
    Custom {
        name: String,
        #[serde(rename = "credentials")]
        credentials: serde_json::Value,
    },
}

// ---------------------------------------------------------------------------
// Task (A2A spec §9.3)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    #[serde(rename = "contextId", skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
    #[serde(rename = "parentTaskId", skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<Message>>,
    pub artifacts: Vec<Artifact>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
}

// ---------------------------------------------------------------------------
// A2aMethod — custom serialization for dot-notation method names
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum A2aMethod {
    TasksSend,
    TasksGet,
    TasksCancel,
    TasksSendStream,
    AgentGetCard,
}

impl A2aMethod {
    fn as_str(&self) -> &'static str {
        match self {
            A2aMethod::TasksSend => "tasks.send",
            A2aMethod::TasksGet => "tasks.get",
            A2aMethod::TasksCancel => "tasks.cancel",
            A2aMethod::TasksSendStream => "tasks.sendStream",
            A2aMethod::AgentGetCard => "agent.getCard",
        }
    }
}

impl fmt::Display for A2aMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<&str> for A2aMethod {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "tasks.send" => Ok(A2aMethod::TasksSend),
            "tasks.get" => Ok(A2aMethod::TasksGet),
            "tasks.cancel" => Ok(A2aMethod::TasksCancel),
            "tasks.sendStream" => Ok(A2aMethod::TasksSendStream),
            "agent.getCard" => Ok(A2aMethod::AgentGetCard),
            _ => Err(format!("unknown A2A method: {s}")),
        }
    }
}

impl Serialize for A2aMethod {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for A2aMethod {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct MethodVisitor;

        impl<'de> Visitor<'de> for MethodVisitor {
            type Value = A2aMethod;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "an A2A method string: \
                     tasks.send, tasks.get, tasks.cancel, \
                     tasks.sendStream, agent.getCard",
                )
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<A2aMethod, E> {
                A2aMethod::try_from(v).map_err(|_| {
                    de::Error::unknown_variant(
                        v,
                        &[
                            "tasks.send",
                            "tasks.get",
                            "tasks.cancel",
                            "tasks.sendStream",
                            "agent.getCard",
                        ],
                    )
                })
            }
        }

        deserializer.deserialize_str(MethodVisitor)
    }
}

// ---------------------------------------------------------------------------
// JsonRpcRequest
// ---------------------------------------------------------------------------

fn default_jsonrpc() -> String {
    "2.0".to_string()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    #[serde(default = "default_jsonrpc")]
    pub jsonrpc: String,
    pub id: String,
    #[serde(rename = "method")]
    pub a2a_method: A2aMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

// ---------------------------------------------------------------------------
// JsonRpcResponse
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

// ---------------------------------------------------------------------------
// JsonRpcNotification
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
}

// ---------------------------------------------------------------------------
// JsonRpcError
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// ---------------------------------------------------------------------------
// Error codes (A2A spec §9.5)
// ---------------------------------------------------------------------------

// Standard JSON-RPC error codes (RFC 4627)
pub const PARSE_ERROR: i32 = -32_700;
pub const INVALID_REQUEST: i32 = -32_600;
pub const METHOD_NOT_FOUND: i32 = -32_601;
pub const INVALID_PARAMS: i32 = -32_602;
pub const INTERNAL_ERROR: i32 = -32_603;

// A2A-specific error codes (spec §9.5)
pub const TASK_NOT_FOUND: i32 = -32_001;
pub const UNSUPPORTED_OPERATION: i32 = -32_002;
pub const CONTENT_TYPE_NOT_SUPPORTED: i32 = -32_003;
pub const TASK_ALREADY_EXISTS: i32 = -32_004;
pub const INVALID_TASK_STATE: i32 = -32_005;
pub const UNKNOWN_ERROR: i32 = -32_099;

// Error status strings (A2A spec §9.5)
pub const STATUS_TASK_NOT_FOUND: &str = "TASK_NOT_FOUND";
pub const STATUS_UNSUPPORTED_OPERATION: &str = "UNSUPPORTED_OPERATION";
pub const STATUS_CONTENT_TYPE_NOT_SUPPORTED: &str = "CONTENT_TYPE_NOT_SUPPORTED";
pub const STATUS_TASK_ALREADY_EXISTS: &str = "TASK_ALREADY_EXISTS";
pub const STATUS_INVALID_TASK_STATE: &str = "INVALID_TASK_STATE";
pub const STATUS_INTERNAL_ERROR: &str = "INTERNAL_ERROR";
pub const STATUS_UNKNOWN_ERROR: &str = "UNKNOWN_ERROR";

// ---------------------------------------------------------------------------
// A2aErrorResponse — error response body (A2A spec §9.5)
// ---------------------------------------------------------------------------

/// Top-level error response body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aErrorResponse {
    pub error: A2aErrorBody,
}

/// Error body with code, status, message, and optional details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aErrorBody {
    pub code: u16,
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<ErrorDetail>,
}

/// A single error detail entry following the Google RPC error-info model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDetail {
    #[serde(rename = "@type")]
    pub at_type: String,
    pub reason: String,
    pub domain: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
}

// ---------------------------------------------------------------------------
// JsonRpcError constructors
// ---------------------------------------------------------------------------

impl JsonRpcError {
    /// Create an `Invalid params` error with details.
    pub fn invalid_params(details: &str) -> Self {
        JsonRpcError {
            code: INVALID_PARAMS,
            message: "Invalid params".to_string(),
            data: Some(serde_json::json!({"details": details})),
        }
    }

    /// Create a `Task not found` error.
    pub fn task_not_found(id: &str) -> Self {
        JsonRpcError {
            code: TASK_NOT_FOUND,
            message: format!("Task not found: {id}"),
            data: None,
        }
    }

    /// Create an `Invalid state transition` error.
    pub fn invalid_state_transition(from: &str, to: &str) -> Self {
        JsonRpcError {
            code: INVALID_TASK_STATE,
            message: format!("Invalid state transition: {from} → {to}"),
            data: None,
        }
    }

    /// Create a `Content type not supported` error.
    pub fn content_type_not_supported(content_type: &str) -> Self {
        JsonRpcError {
            code: CONTENT_TYPE_NOT_SUPPORTED,
            message: "Content type not supported".to_string(),
            data: Some(serde_json::json!({"details": content_type})),
        }
    }

    /// Create an `Unsupported operation` error.
    pub fn unsupported_operation(details: &str) -> Self {
        JsonRpcError {
            code: UNSUPPORTED_OPERATION,
            message: "Unsupported operation".to_string(),
            data: Some(serde_json::json!({"details": details})),
        }
    }

    /// Create an `Internal error`.
    pub fn internal(msg: &str) -> Self {
        JsonRpcError {
            code: INTERNAL_ERROR,
            message: format!("Internal error: {msg}"),
            data: None,
        }
    }
}
