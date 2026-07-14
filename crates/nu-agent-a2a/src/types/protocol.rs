use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

use super::error::{
    CONTENT_TYPE_NOT_SUPPORTED, INTERNAL_ERROR, INVALID_PARAMS, INVALID_TASK_STATE, TASK_NOT_FOUND,
    UNSUPPORTED_OPERATION,
};

// ---------------------------------------------------------------------------
// A2A protocol version (A2A spec §9.2, §14.2)
// ---------------------------------------------------------------------------

/// A2A protocol version sent as the `A2A-Version` HTTP header on every
/// request and response.
pub const A2A_VERSION: &str = "1.0";

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
