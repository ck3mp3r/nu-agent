use std::collections::HashMap;

use super::*;
use chrono::{DateTime, Utc};
use serde_json::json;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixed_time() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).expect("valid timestamp")
}

// ---------------------------------------------------------------------------
// TaskState
// ---------------------------------------------------------------------------

#[test]
fn task_state_all_variants_serde() {
    use serde_test::{Token, assert_tokens};

    let cases: &[(TaskState, &str)] = &[
        (TaskState::Unspecified, "UNSPECIFIED"),
        (TaskState::Submitted, "SUBMITTED"),
        (TaskState::Working, "WORKING"),
        (TaskState::InputRequired, "INPUT_REQUIRED"),
        (TaskState::Completed, "COMPLETED"),
        (TaskState::Failed, "FAILED"),
        (TaskState::Canceled, "CANCELED"),
        (TaskState::Rejected, "REJECTED"),
        (TaskState::AuthRequired, "AUTH_REQUIRED"),
    ];

    for (variant, expected) in cases {
        assert_tokens(
            variant,
            &[Token::UnitVariant {
                name: "TaskState",
                variant: expected,
            }],
        );
    }
}

#[test]
fn task_state_unknown_string_fails_deserialize() {
    let result: Result<TaskState, _> = serde_json::from_str("\"unknown_state\"");
    assert!(result.is_err());
}

#[test]
fn task_state_display_output() {
    assert_eq!(TaskState::Submitted.to_string(), "TASK_STATE_SUBMITTED");
    assert_eq!(TaskState::Working.to_string(), "TASK_STATE_WORKING");
    assert_eq!(TaskState::InputRequired.to_string(), "TASK_STATE_INPUT_REQUIRED");
    assert_eq!(TaskState::Completed.to_string(), "TASK_STATE_COMPLETED");
    assert_eq!(TaskState::Failed.to_string(), "TASK_STATE_FAILED");
    assert_eq!(TaskState::Canceled.to_string(), "TASK_STATE_CANCELED");
    assert_eq!(TaskState::Rejected.to_string(), "TASK_STATE_REJECTED");
}

#[test]
fn task_state_try_from_valid() {
    assert_eq!(
        TaskState::try_from("submitted").unwrap(),
        TaskState::Submitted
    );
    assert_eq!(TaskState::try_from("working").unwrap(), TaskState::Working);
    assert_eq!(
        TaskState::try_from("inputRequired").unwrap(),
        TaskState::InputRequired
    );
    assert_eq!(
        TaskState::try_from("completed").unwrap(),
        TaskState::Completed
    );
    assert_eq!(TaskState::try_from("failed").unwrap(), TaskState::Failed);
    assert_eq!(
        TaskState::try_from("canceled").unwrap(),
        TaskState::Canceled
    );
    assert_eq!(
        TaskState::try_from("rejected").unwrap(),
        TaskState::Rejected
    );
}

#[test]
fn task_state_try_from_invalid_returns_error() {
    let result = TaskState::try_from("bogus_state");
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// TaskStatus
// ---------------------------------------------------------------------------

#[test]
fn task_status_roundtrip_with_message() {
    let status = TaskStatus {
        state: TaskState::Completed,
        timestamp: fixed_time(),
        message: Some(Message {
            role: Role::Agent,
            parts: vec![Part::Text {
                text: "Task completed successfully".to_string(),
            }],
            message_id: uuid::Uuid::new_v4().to_string(),
            extensions: None,
            metadata: None,
        }),
    };

    let json = serde_json::to_value(&status).expect("serialize");
    assert_eq!(json["state"], "COMPLETED");
    assert!(json.get("timestamp").is_some());

    let back: TaskStatus = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, status);
}

#[test]
fn task_status_without_message() {
    let status = TaskStatus {
        state: TaskState::Working,
        timestamp: fixed_time(),
        message: None,
    };

    let json = serde_json::to_value(&status).expect("serialize");
    assert!(
        json.get("message").is_none(),
        "message should be absent when None"
    );

    let back: TaskStatus = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, status);
}

// ---------------------------------------------------------------------------
// Role
// ---------------------------------------------------------------------------

#[test]
fn role_roundtrip() {
    use serde_test::{Token, assert_tokens};

    assert_tokens(
        &Role::User,
        &[Token::UnitVariant {
            name: "Role",
            variant: "USER",
        }],
    );
    assert_tokens(
        &Role::Agent,
        &[Token::UnitVariant {
            name: "Role",
            variant: "AGENT",
        }],
    );
}

// ---------------------------------------------------------------------------
// Part
// ---------------------------------------------------------------------------

#[test]
fn part_text_roundtrip() {
    let part = Part::Text {
        text: "hello world".to_string(),
    };

    let json = serde_json::to_value(&part).expect("serialize");
    assert_eq!(json["text"], "hello world");
    assert!(json.get("type").is_none(), "untagged Part should not have a type key");

    let back: Part = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, part);
}

#[test]
fn part_text_exact_json() {
    let json_str = r#"{"type":"text","text":"hello"}"#;
    let part: Part = serde_json::from_str(json_str).expect("deserialize");
    assert_eq!(
        part,
        Part::Text {
            text: "hello".to_string()
        }
    );
}

#[test]
fn part_file_roundtrip_with_mime_type() {
    let part = Part::File {
        file: FileContent {
            url: "https://example.com/doc.pdf".to_string(),
            filename: "doc.pdf".to_string(),
            media_type: "application/pdf".to_string(),
        },
    };

    let json = serde_json::to_value(&part).expect("serialize");
    assert!(json.get("type").is_none(), "untagged Part should not have a type key");
    assert_eq!(json["file"]["url"], "https://example.com/doc.pdf");
    assert_eq!(json["file"]["mediaType"], "application/pdf");

    let back: Part = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, part);
}

#[test]
fn part_file_roundtrip_without_mime_type() {
    let part = Part::File {
        file: FileContent {
            url: "https://example.com/doc.pdf".to_string(),
            filename: "doc.pdf".to_string(),
            media_type: "application/pdf".to_string(),
        },
    };

    let json = serde_json::to_value(&part).expect("serialize");
    assert!(json.get("type").is_none(), "untagged Part should not have a type key");
    assert_eq!(json["file"]["url"], "https://example.com/doc.pdf");
    assert_eq!(json["file"]["mediaType"], "application/pdf");

    let back: Part = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, part);
}

#[test]
fn part_data_roundtrip() {
    let data = DataContent {
        media_type: "application/json".to_string(),
        schema: json!({"key": "value", "count": 42, "nested": {"a": [1, 2, 3]}}),
    };
    let part = Part::Data { data: data.clone() };

    let json = serde_json::to_value(&part).expect("serialize");
    assert!(json.get("type").is_none(), "untagged Part should not have a type key");
    assert_eq!(json["data"]["mediaType"], "application/json");

    let back: Part = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, part);
}

#[test]
fn part_unknown_type_fails() {
    let result: Result<Part, _> = serde_json::from_str(r#"{"type":"unknown","foo":"bar"}"#);
    assert!(
        result.is_err(),
        "deserializing an unknown Part variant should fail"
    );
}

// ---------------------------------------------------------------------------
// Message
// ---------------------------------------------------------------------------

#[test]
fn message_roundtrip() {
    let msg = Message {
        role: Role::User,
        parts: vec![
            Part::Text {
                text: "Hello".to_string(),
            },
            Part::Data {
                data: DataContent {
                    media_type: "application/json".to_string(),
                    schema: json!({"key": 1}),
                },
            },
        ],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let json = serde_json::to_value(&msg).expect("serialize");
    assert_eq!(json["role"], "USER");
    assert_eq!(json["parts"][0]["text"], "Hello");
    assert!(json["parts"][0].get("type").is_none(), "untagged Part should not have a type key");
    assert_eq!(json["parts"][1]["data"]["mediaType"], "application/json");

    let back: Message = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, msg);
}

#[test]
fn message_exact_json() {
    let json_str = r#"{"role":"AGENT","parts":[{"type":"text","text":"Sure!"}],"messageId":"msg-1"}"#;
    let msg: Message = serde_json::from_str(json_str).expect("deserialize");
    assert_eq!(msg.role, Role::Agent);
    assert_eq!(msg.parts.len(), 1);
    assert_eq!(msg.message_id, "msg-1");
    assert_eq!(
        msg.parts[0],
        Part::Text {
            text: "Sure!".to_string()
        }
    );
}

// ---------------------------------------------------------------------------
// Artifact
// ---------------------------------------------------------------------------

#[test]
fn artifact_full_roundtrip() {
    let artifact = Artifact {
        artifact_id: "art-1".to_string(),
        name: Some("Report".to_string()),
        parts: vec![Part::Text {
            text: "content".to_string(),
        }],
        metadata: Some(HashMap::from([
            ("version".to_string(), json!("1.0")),
            ("size".to_string(), json!(1024)),
        ])),
    };

    let json = serde_json::to_value(&artifact).expect("serialize");
    assert_eq!(json["artifactId"], "art-1");
    assert_eq!(json["name"], "Report");
    assert!(json.get("metadata").is_some());

    let back: Artifact = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, artifact);
}

#[test]
fn artifact_minimal() {
    let artifact = Artifact {
        artifact_id: "art-2".to_string(),
        name: None,
        parts: vec![],
        metadata: None,
    };

    let json = serde_json::to_value(&artifact).expect("serialize");
    assert_eq!(json["artifactId"], "art-2");
    assert!(
        json.get("name").is_none(),
        "name should be absent when None"
    );
    assert_eq!(json["parts"], json!([]));
    assert!(
        json.get("metadata").is_none(),
        "metadata should be absent when None"
    );

    let back: Artifact = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, artifact);
}

// ---------------------------------------------------------------------------
// Task
// ---------------------------------------------------------------------------

#[test]
fn task_full_roundtrip() {
    let task = Task {
        id: "task-1".to_string(),
        context_id: Some("ctx-1".to_string()),
        parent_task_id: Some("parent-1".to_string()),
        session_id: Some("session-1".to_string()),
        status: TaskStatus {
            state: TaskState::Completed,
            timestamp: fixed_time(),
            message: Some(Message {
                role: Role::Agent,
                parts: vec![Part::Text {
                    text: "Done".to_string(),
                }],
                message_id: uuid::Uuid::new_v4().to_string(),
                extensions: None,
                metadata: None,
            }),
        },
        history: Some(vec![Message {
            role: Role::User,
            parts: vec![Part::Text {
                text: "Hi".to_string(),
            }],
            message_id: uuid::Uuid::new_v4().to_string(),
            extensions: None,
            metadata: None,
        }]),
        artifacts: vec![Artifact {
            artifact_id: "art-1".to_string(),
            name: None,
            parts: vec![],
            metadata: None,
        }],
        created_at: None,
        metadata: Some(HashMap::from([("source".to_string(), json!("test"))])),
    };

    let json = serde_json::to_value(&task).expect("serialize");
    assert_eq!(json["id"], "task-1");
    assert_eq!(json["sessionId"], "session-1");
    assert_eq!(json["contextId"], "ctx-1");
    assert_eq!(json["parentTaskId"], "parent-1");
    assert_eq!(json["status"]["state"], "COMPLETED");
    assert!(json.get("history").is_some());
    assert_eq!(json["artifacts"][0]["artifactId"], "art-1");
    assert!(json.get("metadata").is_some());

    let back: Task = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, task);
}

#[test]
fn task_minimal() {
    let task = Task {
        id: "task-2".to_string(),
        context_id: None,
        parent_task_id: None,
        session_id: None,
        status: TaskStatus {
            state: TaskState::Submitted,
            timestamp: fixed_time(),
            message: None,
        },
        history: None,
        artifacts: vec![],
        created_at: None,
        metadata: None,
    };

    let json = serde_json::to_value(&task).expect("serialize");
    assert!(
        json.get("contextId").is_none(),
        "contextId should be absent when None"
    );
    assert!(
        json.get("parentTaskId").is_none(),
        "parentTaskId should be absent when None"
    );
    assert!(json.get("sessionId").is_none());
    assert!(json.get("history").is_none());
    assert!(json.get("metadata").is_none());

    let back: Task = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, task);
}

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

// ---------------------------------------------------------------------------
// A2aMethod
// ---------------------------------------------------------------------------

#[test]
fn a2a_method_all_variants_serialize() {
    use serde_test::{Token, assert_tokens};

    let cases: &[(A2aMethod, &str)] = &[
        (A2aMethod::TasksSend, "tasks.send"),
        (A2aMethod::TasksGet, "tasks.get"),
        (A2aMethod::TasksCancel, "tasks.cancel"),
        (A2aMethod::TasksSendStream, "tasks.sendStream"),
        (A2aMethod::AgentGetCard, "agent.getCard"),
    ];

    for (method, expected) in cases {
        assert_tokens(method, &[Token::Str(expected)]);
    }
}

#[test]
fn a2a_method_display() {
    assert_eq!(A2aMethod::TasksSend.to_string(), "tasks.send");
    assert_eq!(A2aMethod::TasksGet.to_string(), "tasks.get");
    assert_eq!(A2aMethod::TasksCancel.to_string(), "tasks.cancel");
    assert_eq!(A2aMethod::TasksSendStream.to_string(), "tasks.sendStream");
    assert_eq!(A2aMethod::AgentGetCard.to_string(), "agent.getCard");
}

#[test]
fn a2a_method_try_from_valid() {
    assert_eq!(
        A2aMethod::try_from("tasks.send").unwrap(),
        A2aMethod::TasksSend
    );
    assert_eq!(
        A2aMethod::try_from("tasks.get").unwrap(),
        A2aMethod::TasksGet
    );
    assert_eq!(
        A2aMethod::try_from("tasks.cancel").unwrap(),
        A2aMethod::TasksCancel
    );
    assert_eq!(
        A2aMethod::try_from("tasks.sendStream").unwrap(),
        A2aMethod::TasksSendStream
    );
    assert_eq!(
        A2aMethod::try_from("agent.getCard").unwrap(),
        A2aMethod::AgentGetCard
    );
}

#[test]
fn a2a_method_try_from_invalid() {
    assert!(A2aMethod::try_from("unknown.method").is_err());
}

// ---------------------------------------------------------------------------
// JsonRpcRequest
// ---------------------------------------------------------------------------

#[test]
fn json_rpc_request_roundtrip_with_params() {
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: "req-1".to_string(),
        a2a_method: A2aMethod::TasksSend,
        params: Some(json!({
            "id": "task-1",
            "message": {
                "role": "user",
                "parts": [{"type": "text", "text": "hello"}]
            }
        })),
    };

    let json = serde_json::to_value(&req).expect("serialize");
    assert_eq!(json["jsonrpc"], "2.0");
    assert_eq!(json["id"], "req-1");
    assert_eq!(json["method"], "tasks.send");
    assert!(json.get("params").is_some());

    let back: JsonRpcRequest = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, req);
}

#[test]
fn json_rpc_request_roundtrip_without_params() {
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: "req-2".to_string(),
        a2a_method: A2aMethod::AgentGetCard,
        params: None,
    };

    let json = serde_json::to_value(&req).expect("serialize");
    assert_eq!(json["method"], "agent.getCard");
    assert!(json.get("params").is_none());

    let back: JsonRpcRequest = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, req);
}

#[test]
fn json_rpc_request_every_method() {
    let methods = [
        A2aMethod::TasksSend,
        A2aMethod::TasksGet,
        A2aMethod::TasksCancel,
        A2aMethod::TasksSendStream,
        A2aMethod::AgentGetCard,
    ];

    for method in &methods {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "req".to_string(),
            a2a_method: method.clone(),
            params: None,
        };
        let json = serde_json::to_value(&req).expect("serialize");
        let back: JsonRpcRequest = serde_json::from_value(json).expect("deserialize");
        assert_eq!(back.a2a_method, *method);
    }
}

#[test]
fn json_rpc_request_default_jsonrpc() {
    let req: JsonRpcRequest =
        serde_json::from_str(r#"{"id":"r1","method":"tasks.get"}"#).expect("deserialize");
    assert_eq!(req.jsonrpc, "2.0");
    assert_eq!(req.id, "r1");
    assert_eq!(req.a2a_method, A2aMethod::TasksGet);
}

// ---------------------------------------------------------------------------
// JsonRpcResponse
// ---------------------------------------------------------------------------

#[test]
fn json_rpc_response_with_result() {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: "resp-1".to_string(),
        result: Some(json!({"status": "ok"})),
        error: None,
    };

    let json = serde_json::to_value(&resp).expect("serialize");
    assert_eq!(json["result"]["status"], "ok");
    assert!(json.get("error").is_none());

    let back: JsonRpcResponse = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, resp);
}

#[test]
fn json_rpc_response_with_error() {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: "resp-2".to_string(),
        result: None,
        error: Some(JsonRpcError {
            code: -32_000,
            message: "task not found".to_string(),
            data: None,
        }),
    };

    let json = serde_json::to_value(&resp).expect("serialize");
    assert!(json.get("result").is_none());
    assert_eq!(json["error"]["code"], -32_000);

    let back: JsonRpcResponse = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, resp);
}

#[test]
fn json_rpc_response_with_result_and_error() {
    // Per JSON-RPC 2.0 spec, having both result and error is unusual but
    // the struct allows it. Test documents the behavior.
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: "resp-3".to_string(),
        result: Some(json!("partial")),
        error: Some(JsonRpcError {
            code: -32_602,
            message: "Invalid params".to_string(),
            data: None,
        }),
    };

    let json = serde_json::to_value(&resp).expect("serialize");
    assert_eq!(json["result"], "partial");
    assert_eq!(json["error"]["code"], -32_602);

    let back: JsonRpcResponse = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, resp);
}

// ---------------------------------------------------------------------------
// JsonRpcNotification
// ---------------------------------------------------------------------------

#[test]
fn json_rpc_notification_roundtrip() {
    let notif = JsonRpcNotification {
        jsonrpc: "2.0".to_string(),
        method: "tasks.status".to_string(),
        params: json!({"taskId": "t-1", "state": "working"}),
    };

    let json = serde_json::to_value(&notif).expect("serialize");
    assert_eq!(json["jsonrpc"], "2.0");
    assert_eq!(json["method"], "tasks.status");
    assert_eq!(json["params"]["taskId"], "t-1");

    let back: JsonRpcNotification = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, notif);
}
