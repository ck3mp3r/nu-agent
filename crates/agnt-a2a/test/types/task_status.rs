use std::collections::HashMap;

use super::*;
use chrono::{DateTime, Utc};
use serde_json::json;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

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
    assert_eq!(json["state"], "TASK_STATE_COMPLETED");
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
// TaskStatus timestamp format (spec §5.6.1)
// ---------------------------------------------------------------------------

#[test]
fn task_status_timestamp_serializes_with_z_suffix() -> Result<()> {
    // -- Setup & Fixtures
    let status = TaskStatus {
        state: TaskState::Working,
        timestamp: fixed_time(),
        message: None,
    };

    // -- Exec
    let json = serde_json::to_value(&status)?;

    // -- Check
    let ts = json["timestamp"].as_str().ok_or("timestamp string")?;
    assert!(ts.ends_with('Z'), "timestamp must end with 'Z', got: {ts}");
    assert!(
        !ts.contains('+'),
        "timestamp must not contain a timezone offset, got: {ts}"
    );
    assert_eq!(ts, "2023-11-14T22:13:20.000Z");
    Ok(())
}

#[test]
fn task_status_timestamp_deserializes_from_z_suffix() -> Result<()> {
    // -- Setup & Fixtures
    let json = json!({
        "state": "TASK_STATE_WORKING",
        "timestamp": "2023-11-14T22:13:20.000Z"
    });

    // -- Exec
    let status: TaskStatus = serde_json::from_value(json)?;

    // -- Check
    assert_eq!(status.timestamp, fixed_time());
    Ok(())
}

#[test]
fn task_status_timestamp_deserializes_from_offset() -> Result<()> {
    // -- Setup & Fixtures
    let json = json!({
        "state": "TASK_STATE_WORKING",
        "timestamp": "2023-11-14T22:13:20+00:00"
    });

    // -- Exec
    let status: TaskStatus = serde_json::from_value(json)?;

    // -- Check
    assert_eq!(status.timestamp, fixed_time());
    Ok(())
}

#[test]
fn task_status_timestamp_roundtrip_preserves_value() -> Result<()> {
    // -- Setup & Fixtures
    let status = TaskStatus {
        state: TaskState::Completed,
        timestamp: fixed_time(),
        message: None,
    };

    // -- Exec
    let json = serde_json::to_value(&status)?;
    let back: TaskStatus = serde_json::from_value(json)?;

    // -- Check
    assert_eq!(back.timestamp, status.timestamp);
    Ok(())
}

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
    assert_eq!(json["contextId"], "ctx-1");
    assert_eq!(json["parentTaskId"], "parent-1");
    assert_eq!(json["status"]["state"], "TASK_STATE_COMPLETED");
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
    assert!(json.get("history").is_none());
    assert!(json.get("metadata").is_none());

    let back: Task = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, task);
}
