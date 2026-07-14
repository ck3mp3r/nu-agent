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
