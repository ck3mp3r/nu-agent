use super::*;
use chrono::DateTime;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Task::created_at serialization (spec §5.6.1)
// ---------------------------------------------------------------------------

#[test]
fn test_task_created_at_serializes_with_z_suffix() -> Result<()> {
    // -- Setup & Fixtures
    let created_at = DateTime::from_timestamp(1_700_000_000, 0).ok_or("valid timestamp")?;
    let task = Task {
        id: "task-1".to_string(),
        context_id: None,
        parent_task_id: None,
        status: TaskStatus {
            state: TaskState::Working,
            timestamp: created_at,
            message: None,
        },
        history: None,
        artifacts: vec![],
        created_at: Some(created_at),
        metadata: None,
    };

    // -- Exec
    let json = serde_json::to_value(&task)?;

    // -- Check
    let ts = json["created_at"]
        .as_str()
        .ok_or("created_at should be a string")?;
    assert!(ts.ends_with('Z'), "created_at must end with 'Z', got: {ts}");
    assert!(
        !ts.contains('+'),
        "created_at must not contain a timezone offset, got: {ts}"
    );
    assert_eq!(ts, "2023-11-14T22:13:20.000Z");
    Ok(())
}

#[test]
fn test_task_created_at_none_is_omitted() -> Result<()> {
    // -- Setup & Fixtures
    let task = Task {
        id: "task-1".to_string(),
        context_id: None,
        parent_task_id: None,
        status: TaskStatus {
            state: TaskState::Working,
            timestamp: DateTime::from_timestamp(1_700_000_000, 0).ok_or("valid timestamp")?,
            message: None,
        },
        history: None,
        artifacts: vec![],
        created_at: None,
        metadata: None,
    };

    // -- Exec
    let json = serde_json::to_value(&task)?;

    // -- Check
    assert!(
        json.get("created_at").is_none(),
        "created_at should be absent when None"
    );
    Ok(())
}

#[test]
fn test_task_created_at_deserializes_from_offset() -> Result<()> {
    // -- Setup & Fixtures
    let json = serde_json::json!({
        "id": "task-1",
        "status": {
            "state": "TASK_STATE_WORKING",
            "timestamp": "2023-11-14T22:13:20.000Z"
        },
        "artifacts": [],
        "created_at": "2023-11-14T22:13:20+00:00"
    });

    // -- Exec
    let task: Task = serde_json::from_value(json)?;

    // -- Check
    assert_eq!(
        task.created_at,
        Some(DateTime::from_timestamp(1_700_000_000, 0).ok_or("valid timestamp")?)
    );
    Ok(())
}

#[test]
fn test_incoming_task_text_joins_text_parts() -> Result<()> {
    // -- Setup & Fixtures
    let task = IncomingTask {
        task_id: "task-1".to_string(),
        message: Message {
            role: Role::User,
            parts: vec![
                Part::Text {
                    text: "hello".to_string(),
                },
                Part::Text {
                    text: "world".to_string(),
                },
            ],
            message_id: "msg-1".to_string(),
            extensions: None,
            metadata: None,
        },
        sender_url: "http://a.local".to_string(),
        context_id: None,
        parent_task_id: None,
    };

    // -- Exec
    let text = task.text();

    // -- Check
    assert_eq!(text, "hello world");
    Ok(())
}

#[test]
fn test_incoming_task_text_skips_non_text_parts() -> Result<()> {
    // -- Setup & Fixtures
    let task = IncomingTask {
        task_id: "task-1".to_string(),
        message: Message {
            role: Role::User,
            parts: vec![
                Part::Text {
                    text: "only".to_string(),
                },
                Part::Data {
                    data: DataContent {
                        media_type: "application/json".to_string(),
                        schema: serde_json::json!({}),
                    },
                },
            ],
            message_id: "msg-1".to_string(),
            extensions: None,
            metadata: None,
        },
        sender_url: "http://a.local".to_string(),
        context_id: None,
        parent_task_id: None,
    };

    // -- Exec
    let text = task.text();

    // -- Check
    assert_eq!(text, "only");
    Ok(())
}

// ---------------------------------------------------------------------------
// IncomingTask::to_prompt
// ---------------------------------------------------------------------------

#[test]
fn test_incoming_task_to_prompt_with_sender_url() -> Result<()> {
    // -- Setup & Fixtures
    let task = IncomingTask {
        task_id: "task-1".to_string(),
        message: Message {
            role: Role::User,
            parts: vec![Part::Text {
                text: "do work".to_string(),
            }],
            message_id: "msg-1".to_string(),
            extensions: None,
            metadata: None,
        },
        sender_url: "http://a.local".to_string(),
        context_id: None,
        parent_task_id: None,
    };

    // -- Exec
    let prompt = task.to_prompt();

    // -- Check
    assert!(prompt.starts_with("[A2A] do work\n\nProcess this request and respond with your answer. Your response will be automatically delivered as the task result."));
    assert!(prompt.ends_with("\n\n---\nTask ID: task-1\nFrom: http://a.local"));
    Ok(())
}

#[test]
fn test_incoming_task_to_prompt_empty_sender_url() -> Result<()> {
    // -- Setup & Fixtures
    let task = IncomingTask {
        task_id: "task-1".to_string(),
        message: Message {
            role: Role::User,
            parts: vec![Part::Text {
                text: "do work".to_string(),
            }],
            message_id: "msg-1".to_string(),
            extensions: None,
            metadata: None,
        },
        sender_url: String::new(),
        context_id: None,
        parent_task_id: None,
    };

    // -- Exec
    let prompt = task.to_prompt();

    // -- Check
    assert!(prompt.ends_with("\n\n---\nTask ID: task-1"));
    assert!(!prompt.contains("From:"));
    Ok(())
}

#[test]
fn test_incoming_task_to_prompt_with_context_id() -> Result<()> {
    // -- Setup & Fixtures
    let task = IncomingTask {
        task_id: "task-1".to_string(),
        message: Message {
            role: Role::User,
            parts: vec![Part::Text {
                text: "do work".to_string(),
            }],
            message_id: "msg-1".to_string(),
            extensions: None,
            metadata: None,
        },
        sender_url: "http://a.local".to_string(),
        context_id: Some("ctx-abc".to_string()),
        parent_task_id: None,
    };

    // -- Exec
    let prompt = task.to_prompt();

    // -- Check
    assert!(prompt.ends_with("\n\n---\nTask ID: task-1\nFrom: http://a.local\nContext: ctx-abc"));
    Ok(())
}

#[test]
fn test_incoming_task_to_prompt_without_context_id_omits_context_line() -> Result<()> {
    // -- Setup & Fixtures
    let task = IncomingTask {
        task_id: "task-1".to_string(),
        message: Message {
            role: Role::User,
            parts: vec![Part::Text {
                text: "do work".to_string(),
            }],
            message_id: "msg-1".to_string(),
            extensions: None,
            metadata: None,
        },
        sender_url: "http://a.local".to_string(),
        context_id: None,
        parent_task_id: None,
    };

    // -- Exec
    let prompt = task.to_prompt();

    // -- Check
    assert!(!prompt.contains("Context:"));
    Ok(())
}

// ---------------------------------------------------------------------------
// A2aCompletionEvent::to_prompt
// ---------------------------------------------------------------------------

#[test]
fn test_a2a_completion_event_to_prompt() -> Result<()> {
    // -- Setup & Fixtures
    let event = A2aCompletionEvent {
        task_id: "task-2".to_string(),
        agent_name: "agent-b".to_string(),
        status: TaskState::Completed,
        context_id: None,
    };

    // -- Exec
    let prompt = event.to_prompt();

    // -- Check
    assert_eq!(
        prompt,
        "[A2A] Task task-2 by agent-b: COMPLETED. Call tasks_get for details."
    );
    assert!(!prompt.contains("Context:"));
    Ok(())
}

#[test]
fn test_a2a_completion_event_to_prompt_with_context_id() -> Result<()> {
    // -- Setup & Fixtures
    let event = A2aCompletionEvent {
        task_id: "task-2".to_string(),
        agent_name: "agent-b".to_string(),
        status: TaskState::Completed,
        context_id: Some("ctx-abc".to_string()),
    };

    // -- Exec
    let prompt = event.to_prompt();

    // -- Check
    assert_eq!(
        prompt,
        "[A2A] Task task-2 by agent-b: COMPLETED. Call tasks_get for details.\n\nContext: ctx-abc (reuse in tasks_send to continue session, omit for new session)"
    );
    Ok(())
}

#[test]
fn test_a2a_completion_event_to_prompt_truncates_ids() -> Result<()> {
    // -- Setup & Fixtures
    let event = A2aCompletionEvent {
        task_id: "afa1fbf3-1111-2222-3333-444444444444".to_string(),
        agent_name: "lucy".to_string(),
        status: TaskState::Rejected,
        context_id: Some("e92008ee-aaaa-bbbb-cccc-dddddddddddd".to_string()),
    };

    // -- Exec
    let prompt = event.to_prompt();

    // -- Check
    assert_eq!(
        prompt,
        "[A2A] Task afa1fbf3 by lucy: REJECTED. Call tasks_get for details.\n\nContext: e92008ee (reuse in tasks_send to continue session, omit for new session)"
    );
    Ok(())
}

#[test]
fn test_a2a_completion_event_to_prompt_state_labels() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        (TaskState::Completed, "COMPLETED"),
        (TaskState::Rejected, "REJECTED"),
        (TaskState::Failed, "FAILED"),
        (TaskState::Canceled, "CANCELED"),
        (TaskState::AuthRequired, "AUTH_REQUIRED"),
    ];

    // -- Exec & Check
    for (status, expected) in cases {
        let event = A2aCompletionEvent {
            task_id: "task-2".to_string(),
            agent_name: "agent-b".to_string(),
            status,
            context_id: None,
        };
        let prompt = event.to_prompt();
        assert_eq!(
            prompt,
            format!("[A2A] Task task-2 by agent-b: {expected}. Call tasks_get for details.")
        );
    }
    Ok(())
}

#[test]
fn test_a2a_completion_event_to_prompt_rejected() -> Result<()> {
    // -- Setup & Fixtures
    let event = A2aCompletionEvent {
        task_id: "task-2".to_string(),
        agent_name: "agent-b".to_string(),
        status: TaskState::Rejected,
        context_id: None,
    };

    // -- Exec
    let prompt = event.to_prompt();

    // -- Check
    assert!(prompt.contains("REJECTED"));
    assert!(prompt.contains("Call tasks_get for details"));
    assert!(!prompt.contains("COMPLETED"));
    Ok(())
}

#[test]
fn test_a2a_completion_event_to_prompt_failed() -> Result<()> {
    // -- Setup & Fixtures
    let event = A2aCompletionEvent {
        task_id: "task-2".to_string(),
        agent_name: "agent-b".to_string(),
        status: TaskState::Failed,
        context_id: None,
    };

    // -- Exec
    let prompt = event.to_prompt();

    // -- Check
    assert!(prompt.contains("FAILED"));
    assert!(prompt.contains("Call tasks_get for details"));
    assert!(!prompt.contains("COMPLETED"));
    Ok(())
}

#[test]
fn test_a2a_completion_event_context_id_none_stays_none() -> Result<()> {
    // -- Setup & Fixtures
    let event = A2aCompletionEvent {
        task_id: "task-2".to_string(),
        agent_name: "agent-b".to_string(),
        status: TaskState::Completed,
        context_id: None,
    };

    // -- Exec
    let context_id = event.context_id.clone();

    // -- Check
    assert_eq!(context_id, None);
    Ok(())
}

#[test]
fn test_a2a_completion_event_context_id_some_is_preserved() -> Result<()> {
    // -- Setup & Fixtures
    let event = A2aCompletionEvent {
        task_id: "task-2".to_string(),
        agent_name: "agent-b".to_string(),
        status: TaskState::Completed,
        context_id: Some("ctx-abc".to_string()),
    };

    // -- Exec
    let context_id = event.context_id.clone();

    // -- Check
    assert_eq!(context_id.as_deref(), Some("ctx-abc"));
    Ok(())
}
