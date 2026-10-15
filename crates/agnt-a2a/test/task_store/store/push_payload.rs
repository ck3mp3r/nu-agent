use super::*;

#[test]
fn test_task_event_to_stream_response_status_update() -> Result<()> {
    let status = TaskStatus {
        state: TaskState::Working,
        timestamp: chrono::Utc::now(),
        message: None,
    };
    let event = TaskEvent::StatusChanged {
        task_id: "task-1".to_string(),
        status,
    };

    let payload = task_event_to_stream_response(&event, &Some("ctx-abc".to_string()));

    let update = payload
        .get("statusUpdate")
        .ok_or("payload must have a statusUpdate key")?;
    assert_eq!(update["taskId"], "task-1");
    assert_eq!(update["contextId"], "ctx-abc");
    assert_eq!(update["status"]["state"], "TASK_STATE_WORKING");
    assert!(
        payload.get("StatusChanged").is_none(),
        "internal TaskEvent tag must not leak into the payload"
    );
    Ok(())
}

#[test]
fn test_task_event_to_stream_response_artifact_update() -> Result<()> {
    let artifact = Artifact {
        artifact_id: "art-1".to_string(),
        name: Some("result".to_string()),
        parts: vec![Part::Text {
            text: "output".into(),
        }],
        metadata: None,
    };
    let event = TaskEvent::ArtifactAdded {
        task_id: "task-2".to_string(),
        artifact,
    };

    let payload = task_event_to_stream_response(&event, &None);

    let update = payload
        .get("artifactUpdate")
        .ok_or("payload must have an artifactUpdate key")?;
    assert_eq!(update["taskId"], "task-2");
    assert_eq!(update["artifact"]["artifactId"], "art-1");
    assert!(
        update.get("contextId").is_none(),
        "contextId must be omitted when the task has none"
    );
    assert!(
        payload.get("ArtifactAdded").is_none(),
        "internal TaskEvent tag must not leak into the payload"
    );
    Ok(())
}
