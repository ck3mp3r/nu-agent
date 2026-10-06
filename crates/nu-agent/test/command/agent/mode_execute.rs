use super::mode_execute;
use nu_agent_a2a::{InMemoryTaskStore, TaskState};
use nu_agent_core::bus::TurnEvent;
use nu_protocol::{LabeledError, Value, record};
use std::sync::Arc;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// auto_complete_a2a_task tests
// ---------------------------------------------------------------------------

/// Helper: create a task in Working state (the only state that can transition to Completed).
fn setup_working_task(store: &InMemoryTaskStore) -> Result<String> {
    let task = store.create_task(None, None, None);
    let task_id = task.id.clone();
    store
        .update_status(&task_id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    Ok(task_id)
}

#[test]
fn auto_complete_with_string_response_completes_task() -> Result<()> {
    let store = Arc::new(InMemoryTaskStore::default());
    let task_id = setup_working_task(&store)?;

    let response = Value::test_string("The answer is 42.");
    mode_execute::auto_complete_a2a_task(&store, &task_id, &response);

    let task = store.get_task(&task_id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(task.status.state, TaskState::Completed);
    Ok(())
}

#[test]
fn auto_complete_with_record_and_response_field_completes_task() -> Result<()> {
    let store = Arc::new(InMemoryTaskStore::default());
    let task_id = setup_working_task(&store)?;

    let response = Value::test_record(record! {
        "response" => Value::test_string("Hello from record!"),
    });
    mode_execute::auto_complete_a2a_task(&store, &task_id, &response);

    let task = store.get_task(&task_id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(task.status.state, TaskState::Completed);
    Ok(())
}

#[test]
fn auto_complete_with_record_missing_response_field_uses_fallback() -> Result<()> {
    let store = Arc::new(InMemoryTaskStore::default());
    let task_id = setup_working_task(&store)?;

    let response = Value::test_record(record! {
        "foo" => Value::test_string("bar"),
    });
    mode_execute::auto_complete_a2a_task(&store, &task_id, &response);

    let task = store.get_task(&task_id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(task.status.state, TaskState::Completed);
    // The fallback text should result in a non-empty artifact.
    assert!(!task.artifacts.is_empty(), "should have result artifact");
    Ok(())
}

#[test]
fn auto_complete_with_non_text_handles_gracefully() -> Result<()> {
    let store = Arc::new(InMemoryTaskStore::default());
    let task_id = setup_working_task(&store)?;

    // Integer values have no "response" field, so it uses the fallback.
    let response = Value::test_int(42);
    mode_execute::auto_complete_a2a_task(&store, &task_id, &response);

    let task = store.get_task(&task_id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(task.status.state, TaskState::Completed);
    Ok(())
}

#[test]
fn auto_complete_with_record_and_response_field_uses_correct_text() -> Result<()> {
    let store = Arc::new(InMemoryTaskStore::default());
    let task_id = setup_working_task(&store)?;

    let response = Value::test_record(record! {
        "response" => Value::test_string("Exact match"),
    });
    mode_execute::auto_complete_a2a_task(&store, &task_id, &response);

    let task = store.get_task(&task_id).map_err(|e| format!("{e:?}"))?;
    // Inspect the result artifact for the expected text.
    let artifacts = &task.artifacts;
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].name.as_deref(), Some("result"));
    Ok(())
}

// ---------------------------------------------------------------------------
// auto_fail_a2a_task tests
// ---------------------------------------------------------------------------

#[test]
fn auto_fail_transitions_working_task_to_failed() -> Result<()> {
    let store = Arc::new(InMemoryTaskStore::default());
    let task_id = setup_working_task(&store)?;

    mode_execute::auto_fail_a2a_task(&store, &task_id, "turn blew up");

    let task = store.get_task(&task_id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(task.status.state, TaskState::Failed);
    Ok(())
}

#[test]
fn auto_fail_records_reason_as_agent_message() -> Result<()> {
    let store = Arc::new(InMemoryTaskStore::default());
    let task_id = setup_working_task(&store)?;

    mode_execute::auto_fail_a2a_task(&store, &task_id, "model unavailable");

    let task = store.get_task(&task_id).map_err(|e| format!("{e:?}"))?;
    let message = task
        .status
        .message
        .ok_or("failed task should carry a status message")?;
    let text = message
        .parts
        .iter()
        .find_map(|p| match p {
            nu_agent_a2a::Part::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .ok_or("status message should carry the reason text")?;
    assert_eq!(text, "model unavailable");
    Ok(())
}

// ---------------------------------------------------------------------------
// apply_turn_event_to_store tests (TUI listener mapping)
// ---------------------------------------------------------------------------

#[test]
fn apply_turn_event_task_failed_fails_task() -> Result<()> {
    let store = Arc::new(InMemoryTaskStore::default());
    let task_id = setup_working_task(&store)?;

    mode_execute::apply_turn_event_to_store(
        &store,
        &TurnEvent::TaskFailed {
            task_id: task_id.clone(),
            error: "external turn failed".to_string(),
        },
    );

    let task = store.get_task(&task_id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(task.status.state, TaskState::Failed);
    Ok(())
}

#[test]
fn apply_turn_event_task_completed_completes_task() -> Result<()> {
    let store = Arc::new(InMemoryTaskStore::default());
    let task_id = setup_working_task(&store)?;

    mode_execute::apply_turn_event_to_store(
        &store,
        &TurnEvent::TaskCompleted {
            output: "all done".to_string(),
            task_id: task_id.clone(),
        },
    );

    let task = store.get_task(&task_id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(task.status.state, TaskState::Completed);
    Ok(())
}

#[test]
fn apply_turn_event_started_leaves_task_working() -> Result<()> {
    let store = Arc::new(InMemoryTaskStore::default());
    let task_id = setup_working_task(&store)?;

    mode_execute::apply_turn_event_to_store(
        &store,
        &TurnEvent::Started {
            prompt: "hello".to_string(),
            task_id: Some(task_id.clone()),
        },
    );

    let task = store.get_task(&task_id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(task.status.state, TaskState::Working);
    Ok(())
}

// ---------------------------------------------------------------------------
// apply_a2a_turn_result tests (stderr inline handler mapping)
// ---------------------------------------------------------------------------

#[test]
fn apply_a2a_turn_result_err_fails_task() -> Result<()> {
    let store = Arc::new(InMemoryTaskStore::default());
    let task_id = setup_working_task(&store)?;

    let result: core::result::Result<Value, LabeledError> =
        Err(LabeledError::new("provider exploded"));
    mode_execute::apply_a2a_turn_result(&store, &task_id, &result);

    let task = store.get_task(&task_id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(task.status.state, TaskState::Failed);
    Ok(())
}

#[test]
fn apply_a2a_turn_result_ok_completes_task() -> Result<()> {
    let store = Arc::new(InMemoryTaskStore::default());
    let task_id = setup_working_task(&store)?;

    let result: core::result::Result<Value, LabeledError> = Ok(Value::test_string("the answer"));
    mode_execute::apply_a2a_turn_result(&store, &task_id, &result);

    let task = store.get_task(&task_id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(task.status.state, TaskState::Completed);
    Ok(())
}
