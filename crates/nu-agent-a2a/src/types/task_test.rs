use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// IncomingTask::text
// ---------------------------------------------------------------------------

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
        session_id: None,
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
        session_id: None,
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
        session_id: None,
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
        session_id: None,
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

// ---------------------------------------------------------------------------
// A2aCompletionEvent::to_prompt
// ---------------------------------------------------------------------------

#[test]
fn test_a2a_completion_event_to_prompt() -> Result<()> {
    // -- Setup & Fixtures
    let event = A2aCompletionEvent {
        task_id: "task-2".to_string(),
        agent_name: "agent-b".to_string(),
        result: "all done".to_string(),
        status: TaskState::Completed,
    };

    // -- Exec
    let prompt = event.to_prompt();

    // -- Check
    assert!(prompt.starts_with("[A2A] Task completed by agent-b: all done"));
    assert!(prompt.ends_with("\n\n---\nTask ID: task-2\nStatus: TASK_STATE_COMPLETED"));
    Ok(())
}
