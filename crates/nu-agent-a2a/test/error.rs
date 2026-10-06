use super::*;
use serde_json::json;

// ---------------------------------------------------------------------------
// A2aError — Display output for every variant
// ---------------------------------------------------------------------------

#[test]
fn display_connection_refused() {
    let err = A2aError::ConnectionRefused("connection reset by peer".to_string());
    assert_eq!(
        err.to_string(),
        "connection refused: connection reset by peer"
    );
}

#[test]
fn display_timeout() {
    let err = A2aError::Timeout("request exceeded 30s limit".to_string());
    assert_eq!(
        err.to_string(),
        "request timed out: request exceeded 30s limit"
    );
}

#[test]
fn display_json_rpc_error() {
    let err = A2aError::JsonRpcError {
        code: -32_000,
        message: "Task not found".to_string(),
        data: None,
    };
    assert_eq!(
        err.to_string(),
        "JSON-RPC error (code -32000): Task not found"
    );
}

#[test]
fn display_json_rpc_error_with_data() {
    let err = A2aError::JsonRpcError {
        code: -32_602,
        message: "Invalid params".to_string(),
        data: Some(json!({"details": "missing field"})),
    };
    let display = err.to_string();
    assert!(display.contains("JSON-RPC error (code -32602)"));
    assert!(display.contains("Invalid params"));
}

#[test]
fn display_task_not_found() {
    let err = A2aError::TaskNotFound("task-42".to_string());
    assert_eq!(err.to_string(), "task not found: task-42");
}

#[test]
fn display_invalid_state_transition() {
    let err = A2aError::InvalidStateTransition {
        from: TaskState::Completed,
        to: TaskState::Submitted,
    };
    assert_eq!(
        err.to_string(),
        "invalid state transition: Completed → Submitted"
    );
}

#[test]
fn display_serialization_error() {
    let err = A2aError::SerializationError("expected string".to_string());
    assert_eq!(err.to_string(), "serialization error: expected string");
}

#[test]
fn display_internal() {
    let err = A2aError::Internal("unexpected panic".to_string());
    assert_eq!(err.to_string(), "internal error: unexpected panic");
}

// ---------------------------------------------------------------------------
// From<serde_json::Error>
// ---------------------------------------------------------------------------

#[test]
fn from_serde_json_error() {
    let invalid = r#"not valid json"#;
    let json_err: serde_json::Error =
        serde_json::from_str::<serde_json::Value>(invalid).unwrap_err();
    let a2a_err: A2aError = json_err.into();
    assert!(
        a2a_err.to_string().contains("serialization error:"),
        "expected serialization error prefix, got: {}",
        a2a_err
    );
}

// ---------------------------------------------------------------------------
// From<JsonRpcError>
// ---------------------------------------------------------------------------

#[test]
fn from_json_rpc_error() {
    let json_rpc_err = JsonRpcError {
        code: -32_000,
        message: "Task not found".to_string(),
        data: Some(json!({"id": "t-1"})),
    };
    let a2a_err: A2aError = json_rpc_err.into();
    assert!(a2a_err.to_string().contains("JSON-RPC error (code -32000)"));
    assert!(a2a_err.to_string().contains("Task not found"));
}

// ---------------------------------------------------------------------------
// Into<anyhow::Error>
// ---------------------------------------------------------------------------

#[test]
fn into_anyhow_error() {
    let a2a_err = A2aError::Internal("test error".to_string());
    let anyhow_err: anyhow::Error = a2a_err.into();
    assert_eq!(anyhow_err.to_string(), "internal error: test error");
}

// ---------------------------------------------------------------------------
// Debug output
// ---------------------------------------------------------------------------

#[test]
fn debug_output_is_informative() {
    let err = A2aError::TaskNotFound("t-1".to_string());
    let debug = format!("{err:?}");
    // Debug should contain the variant name and the inner value
    assert!(
        debug.contains("TaskNotFound"),
        "Debug output should contain variant name, got: {debug}"
    );
    assert!(
        debug.contains("t-1"),
        "Debug output should contain inner value, got: {debug}"
    );
}

#[test]
fn debug_json_rpc_error() {
    let err = A2aError::JsonRpcError {
        code: -32_000,
        message: "not found".to_string(),
        data: Some(json!({"id": "42"})),
    };
    let debug = format!("{err:?}");
    assert!(debug.contains("JsonRpcError"));
    assert!(debug.contains("-32000"));
    assert!(debug.contains("not found"));
}

// ---------------------------------------------------------------------------
// Clone + PartialEq
// ---------------------------------------------------------------------------

#[test]
fn clone_and_partial_eq() {
    let err = A2aError::Internal("msg".to_string());
    assert_eq!(err, err.clone());
}

#[test]
fn different_variants_not_equal() {
    let a = A2aError::Internal("x".to_string());
    let b = A2aError::Timeout("x".to_string());
    assert_ne!(a, b);
}
