use super::*;

#[test]
fn test_list_tasks_filtered_empty_store() {
    let store = InMemoryTaskStore::default();
    let (tasks, token) = store.list_tasks_filtered(None, None, 50, None);
    assert!(tasks.is_empty());
    assert!(token.is_none());
}

#[test]
fn test_list_tasks_filtered_all() {
    let store = InMemoryTaskStore::default();
    store.create_task(None, None, None);
    store.create_task(None, None, None);
    store.create_task(None, None, None);
    let (tasks, token) = store.list_tasks_filtered(None, None, 50, None);
    assert_eq!(tasks.len(), 3);
    assert!(token.is_none());
}

#[test]
fn test_list_tasks_filtered_by_status() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let t1 = store.create_task(None, None, None);
    store
        .update_status(&t1.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    let _t2 = store.create_task(None, None, None);
    let (working, _) = store.list_tasks_filtered(Some(TaskState::Working), None, 50, None);
    let (submitted, _) = store.list_tasks_filtered(Some(TaskState::Submitted), None, 50, None);
    assert_eq!(working.len(), 1);
    assert_eq!(submitted.len(), 1);
    Ok(())
}

#[test]
fn test_list_tasks_filtered_by_context_id() {
    let store = InMemoryTaskStore::default();
    store.create_task(Some("ctx-a".to_string()), None, None);
    store.create_task(Some("ctx-a".to_string()), None, None);
    store.create_task(Some("ctx-b".to_string()), None, None);

    let (ctx_a, _) = store.list_tasks_filtered(None, Some("ctx-a"), 50, None);
    assert_eq!(ctx_a.len(), 2, "should filter by contextId");

    let (ctx_b, _) = store.list_tasks_filtered(None, Some("ctx-b"), 50, None);
    assert_eq!(ctx_b.len(), 1);

    let (missing, _) = store.list_tasks_filtered(None, Some("ctx-missing"), 50, None);
    assert!(missing.is_empty());
}

#[test]
fn test_list_tasks_filtered_pagination() {
    let store = InMemoryTaskStore::default();
    for _ in 0..10 {
        store.create_task(None, None, None);
    }
    let (page1, token) = store.list_tasks_filtered(None, None, 3, None);
    assert_eq!(page1.len(), 3);
    assert!(token.is_some(), "Should have next page token");
    let (page2, token2) = store.list_tasks_filtered(None, None, 3, token.as_deref());
    assert_eq!(page2.len(), 3);
    assert!(token2.is_some());
}

#[test]
fn test_list_tasks_filtered_pagination_last_page() {
    let store = InMemoryTaskStore::default();
    for _ in 0..3 {
        store.create_task(None, None, None);
    }
    let (page, token) = store.list_tasks_filtered(None, None, 10, None);
    assert_eq!(page.len(), 3);
    assert!(token.is_none(), "No more pages when results <= limit");
}

#[test]
fn test_list_tasks_filtered_sorts_by_status_timestamp_desc() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let oldest = store.create_task(None, None, None);
    let middle = store.create_task(None, None, None);
    let newest = store.create_task(None, None, None);

    // Update the oldest task last so its status timestamp is the most recent.
    store
        .update_status(&middle.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    store
        .update_status(&newest.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    store
        .update_status(&oldest.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;

    let (tasks, _) = store.list_tasks_filtered(None, None, 50, None);
    let ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![oldest.id.as_str(), newest.id.as_str(), middle.id.as_str()],
        "tasks must be sorted by status timestamp descending"
    );
    Ok(())
}

#[test]
fn test_list_tasks_trait_sorts_by_status_timestamp_desc() -> Result<()> {
    let store = InMemoryTaskStore::default();
    let first = store.create_task(None, None, None);
    let second = store.create_task(None, None, None);

    store
        .update_status(&first.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;

    let (tasks, _, _) = TaskStoreBackend::list_tasks(&store, None, None, None);
    assert_eq!(tasks.len(), 2);
    assert_eq!(
        tasks[0].id, first.id,
        "most recently updated task must be first"
    );
    assert_eq!(tasks[1].id, second.id);
    Ok(())
}

#[test]
fn test_list_tasks_filtered_orders_equal_timestamps_by_update_sequence() -> Result<()> {
    // -- Setup & Fixtures
    let iterations = 200;

    // -- Exec & Check
    for _ in 0..iterations {
        let store = InMemoryTaskStore::default();
        let first = store.create_task(None, None, None);
        let second = store.create_task(None, None, None);
        let third = store.create_task(None, None, None);

        // Three rapid updates: when the clock ticks once, the timestamps tie
        // and only the store-assigned update sequence can order the list.
        store
            .update_status(&first.id, TaskState::Working, None)
            .map_err(|e| format!("{e:?}"))?;
        store
            .update_status(&second.id, TaskState::Working, None)
            .map_err(|e| format!("{e:?}"))?;
        store
            .update_status(&third.id, TaskState::Working, None)
            .map_err(|e| format!("{e:?}"))?;

        let (tasks, _) = store.list_tasks_filtered(None, None, 50, None);
        let ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![third.id.as_str(), second.id.as_str(), first.id.as_str()],
            "equal timestamps must order by update sequence descending"
        );
    }
    Ok(())
}
