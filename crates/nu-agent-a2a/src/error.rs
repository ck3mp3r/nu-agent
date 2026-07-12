use serde_json::Value;
use thiserror::Error;

use crate::types::TaskState;

/// Unified error type for all A2A operations.
#[derive(Clone, Debug, PartialEq, Error)]
pub enum A2aError {
    #[error("connection refused: {0}")]
    ConnectionRefused(String),

    #[error("request timed out: {0}")]
    Timeout(String),

    #[error("JSON-RPC error (code {code}): {message}")]
    JsonRpcError {
        code: i32,
        message: String,
        data: Option<Value>,
    },

    #[error("task not found: {0}")]
    TaskNotFound(String),

    #[error("invalid state transition: {from:?} → {to:?}")]
    InvalidStateTransition { from: TaskState, to: TaskState },

    #[error("serialization error: {0}")]
    SerializationError(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl From<serde_json::Error> for A2aError {
    fn from(err: serde_json::Error) -> Self {
        A2aError::SerializationError(err.to_string())
    }
}

impl From<crate::types::JsonRpcError> for A2aError {
    fn from(err: crate::types::JsonRpcError) -> Self {
        A2aError::JsonRpcError {
            code: err.code,
            message: err.message,
            data: err.data,
        }
    }
}
