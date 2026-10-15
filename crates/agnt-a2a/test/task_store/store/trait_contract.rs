use super::*;

#[test]
fn test_trait_contract_immemory() -> Result<()> {
    let store: Box<dyn TaskStoreBackend> = Box::new(InMemoryTaskStore::default());

    // create_task
    let task = store.create_task(None, None, None);
    assert!(!task.id.is_empty(), "Task should have an ID");
    assert_eq!(task.status.state, TaskState::Submitted);

    // get_task
    let retrieved = store.get_task(&task.id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(retrieved.id, task.id);

    // update_status
    let updated = store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(updated.status.state, TaskState::Working);

    // add_artifact
    let artifact = Artifact {
        artifact_id: "art-1".to_string(),
        name: Some("test".to_string()),
        parts: vec![],
        metadata: None,
    };
    let with_artifact = store
        .add_artifact(&task.id, artifact)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(with_artifact.artifacts.len(), 1);

    // list_tasks — no filter
    let (tasks, total, token) = store.list_tasks(None, None, None);
    assert_eq!(tasks.len(), 1, "should list one task");
    assert_eq!(total, 1, "total should match");
    assert!(token.is_none(), "no more pages");

    // list_tasks — with filter
    let (filtered, ftotal, _) = store.list_tasks(Some(vec![TaskState::Working]), None, None);
    assert_eq!(filtered.len(), 1, "should find the working task");
    assert_eq!(ftotal, 1);

    // subscribe
    let rx = store.subscribe(&task.id);
    assert!(!rx.is_closed(), "subscription receiver should be open");
    // Let the receiver drop
    drop(rx);

    // unregister_subscriber
    let rx2 = store.subscribe(&task.id);
    store.unregister_subscriber(&task.id, rx2);

    // append_history
    let msg = Message {
        role: Role::User,
        parts: vec![Part::Text {
            text: "hello".into(),
        }],
        message_id: "msg-1".to_string(),
        extensions: None,
        metadata: None,
    };
    assert!(store.append_history(&task.id, msg).is_ok());

    // create_task_with_idempotency — new key
    let new_task = store
        .create_task_with_idempotency("trait-key-1", None, None, None)
        .map_err(|e| format!("{e:?}"))?;
    assert!(!new_task.id.is_empty());

    // create_task_with_idempotency — duplicate key
    let dup = store.create_task_with_idempotency("trait-key-1", None, None, None);
    assert!(dup.is_err(), "duplicate key should return Err");
    let boxed = dup.unwrap_err();
    let (existing_task, _sender) = *boxed;
    assert_eq!(
        existing_task.id, new_task.id,
        "duplicate should return existing task"
    );
    Ok(())
}
