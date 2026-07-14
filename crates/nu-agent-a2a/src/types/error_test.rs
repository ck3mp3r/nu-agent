use super::*;
use serde_json::json;

// ---------------------------------------------------------------------------
// JsonRpcError
// ---------------------------------------------------------------------------

#[test]
fn json_rpc_error_constants() {
    // Standard JSON-RPC 2.0 codes
    assert_eq!(crate::PARSE_ERROR, -32_700);
    assert_eq!(crate::INVALID_REQUEST, -32_600);
    assert_eq!(crate::METHOD_NOT_FOUND, -32_601);
    assert_eq!(crate::INVALID_PARAMS, -32_602);
    assert_eq!(crate::INTERNAL_ERROR, -32_603);

    // A2A-specific spec codes (§9.5)
    assert_eq!(crate::TASK_NOT_FOUND, -32_001);
    assert_eq!(crate::UNSUPPORTED_OPERATION, -32_002);
    assert_eq!(crate::CONTENT_TYPE_NOT_SUPPORTED, -32_003);
    assert_eq!(crate::TASK_ALREADY_EXISTS, -32_004);
    assert_eq!(crate::INVALID_TASK_STATE, -32_005);
}

#[test]
fn json_rpc_error_new_constructors() {
    let err = JsonRpcError::content_type_not_supported("video/mp4");
    assert_eq!(err.code, -32_003);
    assert_eq!(err.message, "Content type not supported");
    assert_eq!(err.data, Some(serde_json::json!({"details": "video/mp4"})));

    let err = JsonRpcError::unsupported_operation("push notifs disabled");
    assert_eq!(err.code, -32_002);
    assert_eq!(err.message, "Unsupported operation");
    assert_eq!(
        err.data,
        Some(serde_json::json!({"details": "push notifs disabled"}))
    );
}

#[test]
fn json_rpc_error_roundtrip_with_data() {
    let err = JsonRpcError {
        code: -32_000,
        message: "task not found".to_string(),
        data: Some(json!({"taskId": "t-123"})),
    };

    let json = serde_json::to_value(&err).expect("serialize");
    assert_eq!(json["code"], -32_000);
    assert_eq!(json["message"], "task not found");
    assert_eq!(json["data"]["taskId"], "t-123");

    let back: JsonRpcError = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, err);
}

#[test]
fn json_rpc_error_roundtrip_without_data() {
    let err = JsonRpcError {
        code: -32_601,
        message: "method not found".to_string(),
        data: None,
    };

    let json = serde_json::to_value(&err).expect("serialize");
    assert!(json.get("data").is_none());

    let back: JsonRpcError = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, err);
}

#[test]
fn json_rpc_error_invalid_params_constructor() {
    let err = JsonRpcError::invalid_params("missing required field: name");
    assert_eq!(err.code, -32_602);
    assert_eq!(err.message, "Invalid params");
    assert_eq!(
        err.data,
        Some(json!({"details": "missing required field: name"}))
    );
}

#[test]
fn json_rpc_error_task_not_found_constructor() {
    let err = JsonRpcError::task_not_found("t-42");
    assert_eq!(err.code, -32_001);
    assert_eq!(err.message, "Task not found: t-42");
    assert!(err.data.is_none());
}

#[test]
fn json_rpc_error_invalid_state_transition_constructor() {
    let err = JsonRpcError::invalid_state_transition("completed", "working");
    assert_eq!(err.code, -32_005);
    assert_eq!(err.message, "Invalid state transition: completed → working");
    assert!(err.data.is_none());
}

#[test]
fn json_rpc_error_internal_constructor() {
    let err = JsonRpcError::internal("something went wrong");
    assert_eq!(err.code, -32_603);
    assert_eq!(err.message, "Internal error: something went wrong");
    assert!(err.data.is_none());
}
