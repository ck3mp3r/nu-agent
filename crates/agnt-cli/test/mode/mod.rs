use super::*;

use agnt_a2a::{InMemoryTaskStore, Task, TaskState};
use nu_protocol::Span;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// Create a task and move it to `Working`, the state a task is in while a turn
/// runs. Only `Working` tasks can transition to `Completed` or `Failed`.
fn working_task(store: &InMemoryTaskStore) -> Result<Task> {
    let task = store.create_task(None, None, None);
    store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    Ok(task)
}

// endregion: --- Test Support

#[test]
fn test_mode_auto_complete_a2a_task_sets_completed() -> Result<()> {
    // -- Setup & Fixtures
    let store = InMemoryTaskStore::default();
    let task = working_task(&store)?;

    // -- Exec
    auto_complete_a2a_task(&store, &task.id, &Value::string("done", Span::unknown()));

    // -- Check
    let updated = store.get_task(&task.id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(updated.status.state, TaskState::Completed);
    Ok(())
}

#[test]
fn test_mode_auto_fail_a2a_task_sets_failed() -> Result<()> {
    // -- Setup & Fixtures
    let store = InMemoryTaskStore::default();
    let task = working_task(&store)?;

    // -- Exec
    auto_fail_a2a_task(&store, &task.id, "boom");

    // -- Check
    let updated = store.get_task(&task.id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(updated.status.state, TaskState::Failed);
    Ok(())
}

#[test]
fn test_mode_apply_turn_event_to_store_completed_and_failed() -> Result<()> {
    // -- Setup & Fixtures
    let store = InMemoryTaskStore::default();
    let completed = working_task(&store)?;
    let failed = working_task(&store)?;

    // -- Exec
    apply_turn_event_to_store(
        &store,
        &TurnEvent::TaskCompleted {
            output: "ok".to_string(),
            task_id: completed.id.clone(),
        },
    );
    apply_turn_event_to_store(
        &store,
        &TurnEvent::TaskFailed {
            task_id: failed.id.clone(),
            error: "boom".to_string(),
        },
    );

    // -- Check
    assert_eq!(
        store
            .get_task(&completed.id)
            .map_err(|e| format!("{e:?}"))?
            .status
            .state,
        TaskState::Completed
    );
    assert_eq!(
        store
            .get_task(&failed.id)
            .map_err(|e| format!("{e:?}"))?
            .status
            .state,
        TaskState::Failed
    );
    Ok(())
}

#[test]
fn test_mode_apply_turn_event_to_store_ignores_other_events() -> Result<()> {
    // -- Setup & Fixtures
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);

    // -- Exec
    apply_turn_event_to_store(
        &store,
        &TurnEvent::Started {
            prompt: "hi".to_string(),
            task_id: Some(task.id.clone()),
        },
    );

    // -- Check
    assert_eq!(
        store
            .get_task(&task.id)
            .map_err(|e| format!("{e:?}"))?
            .status
            .state,
        TaskState::Submitted
    );
    Ok(())
}

#[test]
fn test_mode_apply_a2a_turn_result_ok_and_err() -> Result<()> {
    // -- Setup & Fixtures
    let store = InMemoryTaskStore::default();
    let ok_task = working_task(&store)?;
    let err_task = working_task(&store)?;
    let ok: crate::error::Result<Value> = Ok(Value::string("done", Span::unknown()));
    let err: crate::error::Result<Value> = Err(crate::error::Error::Custom("nope".to_string()));

    // -- Exec
    apply_a2a_turn_result(&store, &ok_task.id, &ok);
    apply_a2a_turn_result(&store, &err_task.id, &err);

    // -- Check
    assert_eq!(
        store
            .get_task(&ok_task.id)
            .map_err(|e| format!("{e:?}"))?
            .status
            .state,
        TaskState::Completed
    );
    assert_eq!(
        store
            .get_task(&err_task.id)
            .map_err(|e| format!("{e:?}"))?
            .status
            .state,
        TaskState::Failed
    );
    Ok(())
}
