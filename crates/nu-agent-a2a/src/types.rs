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
// TaskState
// ---------------------------------------------------------------------------

/// The state of an A2A task.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskState {
    Submitted,
    Working,
    InputRequired,
    Completed,
    Failed,
    Canceled,
    Rejected,
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TaskState::Submitted => "submitted",
            TaskState::Working => "working",
            TaskState::InputRequired => "inputRequired",
            TaskState::Completed => "completed",
            TaskState::Failed => "failed",
            TaskState::Canceled => "canceled",
            TaskState::Rejected => "rejected",
        };
        f.write_str(s)
    }
}

impl TryFrom<&str> for TaskState {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
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
// TaskStatus
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskStatus {
    pub state: TaskState,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// ---------------------------------------------------------------------------
// Role
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Agent,
}

// ---------------------------------------------------------------------------
// Part
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Part {
    Text {
        text: String,
    },
    File {
        url: String,
        #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
    },
    Data {
        data: Value,
    },
}

// ---------------------------------------------------------------------------
// Message
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
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
// Artifact
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
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
// Task
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

// JSON-RPC 2.0 standard error codes (spec §9.5)
pub const PARSE_ERROR: i32 = -32_700;
pub const INVALID_REQUEST: i32 = -32_600;
pub const METHOD_NOT_FOUND: i32 = -32_601;
pub const INVALID_PARAMS: i32 = -32_602;
pub const INTERNAL_ERROR: i32 = -32_603;

// A2A-specific error codes (spec §9.5)
pub const TASK_NOT_FOUND: i32 = -32_001;
pub const TASK_NOT_SUPPORTED: i32 = -32_000;
pub const CONTENT_TYPE_NOT_SUPPORTED: i32 = -32_002;
pub const UNSUPPORTED_OPERATION: i32 = -32_003;
pub const PUSH_NOTIFICATION_NOT_SUPPORTED: i32 = -32_004;

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
            code: TASK_NOT_SUPPORTED,
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
