use super::*;

#[test]
fn test_create_task() {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    assert!(!task.id.is_empty(), "Task should have an ID");
    // UUID format: 8-4-4-4-12 hex chars
    assert_eq!(task.id.len(), 36, "UUID should be 36 chars");
    assert_eq!(task.status.state, TaskState::Submitted);
    assert!(task.context_id.is_none());
    assert!(task.parent_task_id.is_none());
}

#[test]
fn test_create_task_with_context_id() {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(Some("ctx-1".to_string()), None, None);
    assert_eq!(task.context_id, Some("ctx-1".to_string()));
    assert!(task.parent_task_id.is_none());
}

#[test]
fn test_create_task_with_parent_task_id() {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, Some("parent-1".to_string()), None);
    assert_eq!(task.parent_task_id, Some("parent-1".to_string()));
    assert!(task.context_id.is_none());
}

#[test]
fn test_create_task_with_all_options() {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(
        Some("ctx-1".to_string()),
        Some("parent-1".to_string()),
        None,
    );
    assert_eq!(task.context_id, Some("ctx-1".to_string()));
    assert_eq!(task.parent_task_id, Some("parent-1".to_string()));
}

#[test]
fn test_get_task_returns_created() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let created = store.create_task(None, None, None);
    let retrieved = store.get_task(&created.id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(retrieved.id, created.id);
    assert_eq!(retrieved.status.state, created.status.state);
    Ok(())
}

#[test]
fn test_get_task_not_found() {
    let store = InMemoryTaskStore::default();
    let result = store.get_task("nonexistent-id");
    assert!(matches!(result, Err(A2aError::TaskNotFound(_))));
}

#[test]
fn test_update_status_valid() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    let updated = store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(updated.status.state, TaskState::Working);
    Ok(())
}

#[test]
fn test_update_status_invalid() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    let result = store.update_status(&task.id, TaskState::Submitted, None);
    assert!(matches!(
        result,
        Err(A2aError::InvalidStateTransition { .. })
    ));
    Ok(())
}

#[test]
fn test_cancel_task() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    let canceled = store.cancel_task(&task.id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(canceled.status.state, TaskState::Canceled);
    Ok(())
}

#[test]
fn test_cancel_completed_fails() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    store
        .update_status(&task.id, TaskState::Completed, None)
        .map_err(|e| format!("{e:?}"))?;
    let result = store.cancel_task(&task.id);
    assert!(result.is_err(), "Cannot cancel a completed task");
    Ok(())
}

#[test]
fn test_add_artifact() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);
    let artifact = Artifact {
        artifact_id: "art-1".to_string(),
        name: Some("result.txt".to_string()),
        parts: vec![],
        metadata: None,
    };
    let updated = store
        .add_artifact(&task.id, artifact.clone())
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(updated.artifacts.len(), 1);
    assert_eq!(updated.artifacts[0].artifact_id, "art-1");
    Ok(())
}

#[test]
fn test_list_tasks() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let t1 = store.create_task(None, None, None);
    let _t2 = store.create_task(None, None, None);
    let t3 = store.create_task(None, None, None);
    // Move t1 and t3 to Working
    store
        .update_status(&t1.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    store
        .update_status(&t3.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;

    let all = store.list_tasks(None);
    assert_eq!(all.len(), 3);

    let working = store.list_tasks(Some(TaskState::Working));
    assert_eq!(working.len(), 2);

    let submitted = store.list_tasks(Some(TaskState::Submitted));
    assert_eq!(submitted.len(), 1);
    Ok(())
}
