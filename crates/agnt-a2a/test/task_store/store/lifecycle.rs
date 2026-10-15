use super::*;

#[test]
fn test_complete_task() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;

    let completed = store
        .complete_task(&task.id, "result data")
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(completed.status.state, TaskState::Completed);
    assert_eq!(completed.artifacts.len(), 1);
    assert_eq!(
        completed.artifacts[0].parts[0],
        Part::Text {
            text: "result data".into()
        }
    );
    Ok(())
}

#[test]
fn test_complete_submitted_task_fails() {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    let result = store.complete_task(&task.id, "data");
    assert!(result.is_err(), "Cannot complete a Submitted task directly");
}

#[test]
fn test_reject_task_from_submitted() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);

    let rejected = store
        .reject_task(&task.id, "busy")
        .map_err(|e| format!("{e:?}"))?;

    assert_eq!(rejected.status.state, TaskState::Rejected);
    let message = rejected.status.message.ok_or("should have a message")?;
    assert_eq!(message.role, Role::Agent);
    assert_eq!(
        message.parts,
        vec![Part::Text {
            text: "busy".into()
        }]
    );
    Ok(())
}

#[test]
fn test_reject_task_from_working() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;

    let rejected = store
        .reject_task(&task.id, "busy")
        .map_err(|e| format!("{e:?}"))?;

    assert_eq!(rejected.status.state, TaskState::Rejected);
    let message = rejected.status.message.ok_or("should have a message")?;
    assert_eq!(message.role, Role::Agent);
    assert_eq!(
        message.parts,
        vec![Part::Text {
            text: "busy".into()
        }]
    );
    Ok(())
}

#[test]
fn test_fail_task_from_working() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;

    let failed = store
        .fail_task(&task.id, "turn failed")
        .map_err(|e| format!("{e:?}"))?;

    assert_eq!(failed.status.state, TaskState::Failed);
    let message = failed.status.message.ok_or("should have a message")?;
    assert_eq!(message.role, Role::Agent);
    assert_eq!(
        message.parts,
        vec![Part::Text {
            text: "turn failed".into()
        }]
    );
    Ok(())
}

#[test]
fn test_fail_task_on_terminal_state_fails() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    store
        .complete_task(&task.id, "done")
        .map_err(|e| format!("{e:?}"))?;

    let result = store.fail_task(&task.id, "too late");
    assert!(matches!(
        result,
        Err(A2aError::InvalidStateTransition { .. })
    ));
    let unchanged = store.get_task(&task.id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(unchanged.status.state, TaskState::Completed);
    Ok(())
}

#[test]
fn test_reject_task_on_terminal_state_fails() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    store
        .complete_task(&task.id, "done")
        .map_err(|e| format!("{e:?}"))?;

    let result = store.reject_task(&task.id, "too late");
    assert!(matches!(
        result,
        Err(A2aError::InvalidStateTransition { .. })
    ));
    let unchanged = store.get_task(&task.id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(unchanged.status.state, TaskState::Completed);
    Ok(())
}
