//! The turn→terminal mapping and its two drivers: the store transition, the
//! row it publishes, and the listener's delegation.

use super::*;

use agnt_a2a::{Part, Role};

/// Create a task and move it to `Working`, the state a task is in while a turn
/// runs. Only `Working` tasks can transition to `Completed` or `Failed`.
fn working_task(store: &InMemoryTaskStore) -> Result<Task> {
    let task = store.create_task(None, None, None);
    store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    Ok(task)
}

/// The text of a message's parts, joined.
fn parts_text(parts: &[Part]) -> String {
    parts
        .iter()
        .filter_map(|part| match part {
            Part::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// The text of the `result` artifact the store recorded for a completed task.
fn result_text(store: &InMemoryTaskStore, task_id: &str) -> Result<String> {
    let task = store.get_task(task_id).map_err(|e| format!("{e:?}"))?;
    let artifact = task
        .artifacts
        .iter()
        .find(|artifact| artifact.name.as_deref() == Some("result"))
        .ok_or("the completed task should carry a result artifact")?;
    Ok(parts_text(&artifact.parts))
}

/// The text the store recorded as the task's status message.
fn status_text(store: &InMemoryTaskStore, task_id: &str) -> Result<String> {
    let task = store.get_task(task_id).map_err(|e| format!("{e:?}"))?;
    let message = task
        .status
        .message
        .ok_or("the status should carry a message")?;
    Ok(parts_text(&message.parts))
}

/// WHEN a one-shot turn succeeds, THE turn→terminal mapping SHALL complete the
/// task with the response text and publish one `TaskCompleted` row for it.
#[tokio::test]
async fn test_mode_apply_turn_terminal_completed_transitions_and_publishes() -> Result<()> {
    // -- Setup & Fixtures
    let store = InMemoryTaskStore::default();
    let task = working_task(&store)?;
    let bus = agnt_runtime::bus::Bus::default();
    let mut lifecycle_rx = bus.lifecycle().subscribe();
    let result: crate::error::Result<Value> = Ok(Value::string("done", Span::unknown()));

    // -- Exec
    apply_turn_terminal(
        &store,
        bus.lifecycle(),
        &task.id,
        TaskTerminal::from_result(&result),
    );

    // -- Check
    let updated = store.get_task(&task.id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(updated.status.state, TaskState::Completed);
    assert_eq!(
        result_text(&store, &task.id)?,
        "done",
        "the completed task records the response text as its result artifact"
    );
    let Ok(event) = lifecycle_rx.try_recv() else {
        return Err("the mapping must publish one task-terminal row".into());
    };
    assert_eq!(
        event,
        LifecycleEvent::TaskCompleted {
            task_id: task.id.clone(),
            output: "done".to_string(),
        },
        "the published row carries the task id and the response text"
    );
    assert!(
        lifecycle_rx.try_recv().is_err(),
        "the mapping publishes exactly one row"
    );
    Ok(())
}

/// WHEN a one-shot turn fails, THE turn→terminal mapping SHALL fail the task
/// with the error text and publish one `TaskFailed` row for it.
#[tokio::test]
async fn test_mode_apply_turn_terminal_failed_transitions_and_publishes() -> Result<()> {
    // -- Setup & Fixtures
    let store = InMemoryTaskStore::default();
    let task = working_task(&store)?;
    let bus = agnt_runtime::bus::Bus::default();
    let mut lifecycle_rx = bus.lifecycle().subscribe();
    let result: crate::error::Result<Value> = Err(crate::error::Error::Custom("nope".to_string()));

    // -- Exec
    apply_turn_terminal(
        &store,
        bus.lifecycle(),
        &task.id,
        TaskTerminal::from_result(&result),
    );

    // -- Check
    let updated = store.get_task(&task.id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(updated.status.state, TaskState::Failed);
    assert_eq!(status_text(&store, &task.id)?, "nope");
    let Ok(event) = lifecycle_rx.try_recv() else {
        return Err("the mapping must publish one task-terminal row".into());
    };
    assert_eq!(
        event,
        LifecycleEvent::TaskFailed {
            task_id: task.id.clone(),
            error: "nope".to_string(),
        },
        "the published row carries the task id and the error text"
    );
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
        &LifecycleEvent::TaskCompleted {
            output: "ok".to_string(),
            task_id: completed.id.clone(),
        },
    );
    apply_turn_event_to_store(
        &store,
        &LifecycleEvent::TaskFailed {
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

/// IF both drivers ever transition the same task, THEN THE store SHALL keep the
/// terminal state it already reached and THE transition SHALL stay benign: the
/// store error is logged, never propagated.
#[test]
fn test_mode_apply_turn_event_to_store_repeated_row_is_benign() -> Result<()> {
    // -- Setup & Fixtures
    let store = InMemoryTaskStore::default();
    let task = working_task(&store)?;
    let row = LifecycleEvent::TaskCompleted {
        output: "done".to_string(),
        task_id: task.id.clone(),
    };

    // -- Exec: the loop's listener and the in-hand path both report the outcome.
    apply_turn_event_to_store(&store, &row);
    apply_turn_event_to_store(&store, &row);

    // -- Check
    let updated = store.get_task(&task.id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        updated.status.state,
        TaskState::Completed,
        "a repeated row must not disturb the terminal state"
    );
    Ok(())
}

/// IF the session attach for an external task fails, THEN THE write-back
/// listener SHALL fail the task with the attach error text, leaving no task
/// stranded in `Working`.
#[test]
fn test_mode_apply_turn_event_to_store_attach_failure_fails_the_task() -> Result<()> {
    // -- Setup & Fixtures: the state a boundary task is in when it waits.
    let store = InMemoryTaskStore::default();
    let task = working_task(&store)?;

    // -- Exec: the row the router publishes when the attach cannot run.
    apply_turn_event_to_store(
        &store,
        &LifecycleEvent::TaskFailed {
            task_id: task.id.clone(),
            error: "session store unavailable".to_string(),
        },
    );

    // -- Check
    let updated = store.get_task(&task.id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        updated.status.state,
        TaskState::Failed,
        "a failed attach must reach a terminal state"
    );
    let message = updated
        .status
        .message
        .ok_or("the fail reason should be recorded")?;
    assert_eq!(message.role, Role::Agent);
    assert_eq!(
        message.parts,
        vec![Part::Text {
            text: "session store unavailable".to_string()
        }],
        "the store records the attach failure's own text"
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
        &LifecycleEvent::TurnStarted {
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
