use super::*;

#[test]
fn test_create_task_with_idempotency_new_key() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task = store
        .create_task_with_idempotency("key-1", None, None, None)
        .map_err(|e| format!("{e:?}"))?;
    assert!(!task.id.is_empty(), "Task should have an ID");
    assert_eq!(task.status.state, TaskState::Submitted);
    Ok(())
}

#[test]
fn test_create_task_with_idempotency_same_key_returns_same() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task1 = store
        .create_task_with_idempotency("key-dup", None, None, None)
        .map_err(|e| format!("{e:?}"))?;

    // Second call with same key should return Err with existing task
    let result = store.create_task_with_idempotency("key-dup", None, None, None);
    match result {
        Err(boxed) => {
            let (existing, is_dup) = *boxed;
            assert!(is_dup, "should be marked as duplicate");
            assert_eq!(existing.id, task1.id, "should return same task");
        }
        Ok(_) => panic!("expected duplicate error"),
    }
    Ok(())
}

#[test]
fn test_create_task_with_idempotency_different_keys_create_different_tasks() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let task1 = store
        .create_task_with_idempotency("key-a", None, None, None)
        .map_err(|e| format!("{e:?}"))?;
    let task2 = store
        .create_task_with_idempotency("key-b", None, None, None)
        .map_err(|e| format!("{e:?}"))?;

    assert_ne!(
        task1.id, task2.id,
        "different keys should create different tasks"
    );
    assert_eq!(task1.status.state, TaskState::Submitted);
    assert_eq!(task2.status.state, TaskState::Submitted);
    Ok(())
}
