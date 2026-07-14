use super::{InMemoryTaskStore, TaskStoreBackend};
use crate::{
    A2aError, Artifact, Message, Part, PushAuthScheme, PushAuthenticationInfo, Role, TaskEvent,
    TaskState,
};

// ---------------------------------------------------------------------------
// InMemoryTaskStore
// ---------------------------------------------------------------------------

#[test]
fn test_create_task() {
    let store = InMemoryTaskStore::new();
    let task = store.create_task(None, None, None, None);
    assert!(!task.id.is_empty(), "Task should have an ID");
    // UUID format: 8-4-4-4-12 hex chars
    assert_eq!(task.id.len(), 36, "UUID should be 36 chars");
    assert_eq!(task.status.state, TaskState::Submitted);
    assert!(task.session_id.is_none());
    assert!(task.context_id.is_none());
    assert!(task.parent_task_id.is_none());
}

#[test]
fn test_create_task_with_session() {
    let store = InMemoryTaskStore::new();
    let task = store.create_task(Some("sess-1".to_string()), None, None, None);
    assert_eq!(task.session_id, Some("sess-1".to_string()));
    assert!(task.context_id.is_none());
    assert!(task.parent_task_id.is_none());
}

#[test]
fn test_create_task_with_context_id() {
    let store = InMemoryTaskStore::new();
    let task = store.create_task(None, Some("ctx-1".to_string()), None, None);
    assert_eq!(task.context_id, Some("ctx-1".to_string()));
    assert!(task.session_id.is_none());
    assert!(task.parent_task_id.is_none());
}

#[test]
fn test_create_task_with_parent_task_id() {
    let store = InMemoryTaskStore::new();
    let task = store.create_task(None, None, Some("parent-1".to_string()), None);
    assert_eq!(task.parent_task_id, Some("parent-1".to_string()));
    assert!(task.session_id.is_none());
    assert!(task.context_id.is_none());
}

#[test]
fn test_create_task_with_all_options() {
    let store = InMemoryTaskStore::new();
    let task = store.create_task(
        Some("sess-1".to_string()),
        Some("ctx-1".to_string()),
        Some("parent-1".to_string()),
        None,
    );
    assert_eq!(task.session_id, Some("sess-1".to_string()));
    assert_eq!(task.context_id, Some("ctx-1".to_string()));
    assert_eq!(task.parent_task_id, Some("parent-1".to_string()));
}

#[test]
fn test_get_task_returns_created() {
    let store = InMemoryTaskStore::new();
    let created = store.create_task(None, None, None, None);
    let retrieved = store.get_task(&created.id).unwrap();
    assert_eq!(retrieved.id, created.id);
    assert_eq!(retrieved.status.state, created.status.state);
}

#[test]
fn test_get_task_not_found() {
    let store = InMemoryTaskStore::new();
    let result = store.get_task("nonexistent-id");
    assert!(matches!(result, Err(A2aError::TaskNotFound(_))));
}

#[test]
fn test_update_status_valid() {
    let store = InMemoryTaskStore::new();
    let task = store.create_task(None, None, None, None);
    let updated = store
        .update_status(&task.id, TaskState::Working, None)
        .unwrap();
    assert_eq!(updated.status.state, TaskState::Working);
}

#[test]
fn test_update_status_invalid() {
    let store = InMemoryTaskStore::new();
    let task = store.create_task(None, None, None, None);
    store
        .update_status(&task.id, TaskState::Working, None)
        .unwrap();
    let result = store.update_status(&task.id, TaskState::Submitted, None);
    assert!(matches!(
        result,
        Err(A2aError::InvalidStateTransition { .. })
    ));
}

#[test]
fn test_cancel_task() {
    let store = InMemoryTaskStore::new();
    let task = store.create_task(None, None, None, None);
    let canceled = store.cancel_task(&task.id).unwrap();
    assert_eq!(canceled.status.state, TaskState::Canceled);
}

#[test]
fn test_cancel_completed_fails() {
    let store = InMemoryTaskStore::new();
    let task = store.create_task(None, None, None, None);
    store
        .update_status(&task.id, TaskState::Working, None)
        .unwrap();
    store
        .update_status(&task.id, TaskState::Completed, None)
        .unwrap();
    let result = store.cancel_task(&task.id);
    assert!(result.is_err(), "Cannot cancel a completed task");
}

#[test]
fn test_add_artifact() {
    let store = InMemoryTaskStore::new();
    let task = store.create_task(None, None, None, None);
    let artifact = Artifact {
        artifact_id: "art-1".to_string(),
        name: Some("result.txt".to_string()),
        parts: vec![],
        metadata: None,
    };
    let updated = store.add_artifact(&task.id, artifact.clone()).unwrap();
    assert_eq!(updated.artifacts.len(), 1);
    assert_eq!(updated.artifacts[0].artifact_id, "art-1");
}

#[test]
fn test_list_tasks() {
    let store = InMemoryTaskStore::new();
    let t1 = store.create_task(None, None, None, None);
    let _t2 = store.create_task(None, None, None, None);
    let t3 = store.create_task(None, None, None, None);
    // Move t1 and t3 to Working
    store
        .update_status(&t1.id, TaskState::Working, None)
        .unwrap();
    store
        .update_status(&t3.id, TaskState::Working, None)
        .unwrap();

    let all = store.list_tasks(None);
    assert_eq!(all.len(), 3);

    let working = store.list_tasks(Some(TaskState::Working));
    assert_eq!(working.len(), 2);

    let submitted = store.list_tasks(Some(TaskState::Submitted));
    assert_eq!(submitted.len(), 1);
}

// ---------------------------------------------------------------------------
// Idempotency
// ---------------------------------------------------------------------------

#[test]
fn test_create_task_with_idempotency_new_key() {
    let store = InMemoryTaskStore::new();
    let task = store
        .create_task_with_idempotency("key-1", None, None, None, None)
        .unwrap();
    assert!(!task.id.is_empty(), "Task should have an ID");
    assert_eq!(task.status.state, TaskState::Submitted);
}

#[test]
fn test_create_task_with_idempotency_same_key_returns_same() {
    let store = InMemoryTaskStore::new();
    let task1 = store
        .create_task_with_idempotency("key-dup", None, None, None, None)
        .unwrap();

    // Second call with same key should return Err with existing task
    let result = store.create_task_with_idempotency("key-dup", None, None, None, None);
    match result {
        Err(boxed) => {
            let (existing, is_dup) = *boxed;
            assert!(is_dup, "should be marked as duplicate");
            assert_eq!(existing.id, task1.id, "should return same task");
        }
        Ok(_) => panic!("expected duplicate error"),
    }
}

#[test]
fn test_create_task_with_idempotency_different_keys_create_different_tasks() {
    let store = InMemoryTaskStore::new();
    let task1 = store
        .create_task_with_idempotency("key-a", None, None, None, None)
        .unwrap();
    let task2 = store
        .create_task_with_idempotency("key-b", None, None, None, None)
        .unwrap();

    assert_ne!(
        task1.id, task2.id,
        "different keys should create different tasks"
    );
    assert_eq!(task1.status.state, TaskState::Submitted);
    assert_eq!(task2.status.state, TaskState::Submitted);
}

// ---------------------------------------------------------------------------
// is_valid_transition
// ---------------------------------------------------------------------------

#[test]
fn test_is_valid_transition_all_valid() {
    // Test every valid transition
    assert!(super::is_valid_transition(
        &TaskState::Submitted,
        &TaskState::Working
    ));
    assert!(super::is_valid_transition(
        &TaskState::Submitted,
        &TaskState::Canceled
    ));
    assert!(super::is_valid_transition(
        &TaskState::Submitted,
        &TaskState::Rejected
    ));
    assert!(super::is_valid_transition(
        &TaskState::Working,
        &TaskState::InputRequired
    ));
    assert!(super::is_valid_transition(
        &TaskState::Working,
        &TaskState::Completed
    ));
    assert!(super::is_valid_transition(
        &TaskState::Working,
        &TaskState::Failed
    ));
    assert!(super::is_valid_transition(
        &TaskState::Working,
        &TaskState::Canceled
    ));
    assert!(super::is_valid_transition(
        &TaskState::InputRequired,
        &TaskState::Working
    ));
    assert!(super::is_valid_transition(
        &TaskState::InputRequired,
        &TaskState::Canceled
    ));
}

#[test]
fn test_is_valid_transition_all_invalid() {
    // Terminal states reject everything
    for terminal in &[
        TaskState::Completed,
        TaskState::Failed,
        TaskState::Canceled,
        TaskState::Rejected,
    ] {
        for target in &[
            TaskState::Submitted,
            TaskState::Working,
            TaskState::InputRequired,
            TaskState::Completed,
            TaskState::Failed,
            TaskState::Canceled,
            TaskState::Rejected,
        ] {
            assert!(
                !super::is_valid_transition(terminal, target),
                "{:?} -> {:?} should be invalid",
                terminal,
                target
            );
        }
    }
    // Explicit invalid: Working -> Submitted
    assert!(!super::is_valid_transition(
        &TaskState::Working,
        &TaskState::Submitted
    ));
}

#[test]
fn test_complete_task() {
    let store = InMemoryTaskStore::new();
    let task = store.create_task(None, None, None, None);
    store
        .update_status(&task.id, TaskState::Working, None)
        .unwrap();

    let completed = store.complete_task(&task.id, "result data").unwrap();
    assert_eq!(completed.status.state, TaskState::Completed);
    assert_eq!(completed.artifacts.len(), 1);
    assert_eq!(
        completed.artifacts[0].parts[0],
        Part::Text {
            text: "result data".into()
        }
    );
}

#[test]
fn test_complete_submitted_task_fails() {
    let store = InMemoryTaskStore::new();
    let task = store.create_task(None, None, None, None);
    let result = store.complete_task(&task.id, "data");
    assert!(result.is_err(), "Cannot complete a Submitted task directly");
}

#[test]
fn test_concurrent_writes() {
    use std::sync::Arc;
    use std::thread;

    let store = Arc::new(InMemoryTaskStore::new());
    let mut handles = vec![];

    for _ in 0..10 {
        let s = store.clone();
        handles.push(thread::spawn(move || {
            s.create_task(None, None, None, None);
        }));
    }

    for h in handles {
        h.join().expect("Thread panicked");
    }

    assert_eq!(
        store.list_tasks(None).len(),
        10,
        "All 10 concurrent creates should succeed"
    );
}

// ---------------------------------------------------------------------------
// list_tasks_filtered
// ---------------------------------------------------------------------------

#[test]
fn test_list_tasks_filtered_empty_store() {
    let store = InMemoryTaskStore::new();
    let (tasks, token) = store.list_tasks_filtered(None, 50, None);
    assert!(tasks.is_empty());
    assert!(token.is_none());
}

#[test]
fn test_list_tasks_filtered_all() {
    let store = InMemoryTaskStore::new();
    store.create_task(None, None, None, None);
    store.create_task(None, None, None, None);
    store.create_task(None, None, None, None);
    let (tasks, token) = store.list_tasks_filtered(None, 50, None);
    assert_eq!(tasks.len(), 3);
    assert!(token.is_none());
}

#[test]
fn test_list_tasks_filtered_by_status() {
    let store = InMemoryTaskStore::new();
    let t1 = store.create_task(None, None, None, None);
    store
        .update_status(&t1.id, TaskState::Working, None)
        .unwrap();
    let _t2 = store.create_task(None, None, None, None);
    let (working, _) = store.list_tasks_filtered(Some(TaskState::Working), 50, None);
    let (submitted, _) = store.list_tasks_filtered(Some(TaskState::Submitted), 50, None);
    assert_eq!(working.len(), 1);
    assert_eq!(submitted.len(), 1);
}

#[test]
fn test_list_tasks_filtered_pagination() {
    let store = InMemoryTaskStore::new();
    for _ in 0..10 {
        store.create_task(None, None, None, None);
    }
    let (page1, token) = store.list_tasks_filtered(None, 3, None);
    assert_eq!(page1.len(), 3);
    assert!(token.is_some(), "Should have next page token");
    let (page2, token2) = store.list_tasks_filtered(None, 3, token.as_deref());
    assert_eq!(page2.len(), 3);
    assert!(token2.is_some());
}

#[test]
fn test_list_tasks_filtered_pagination_last_page() {
    let store = InMemoryTaskStore::new();
    for _ in 0..3 {
        store.create_task(None, None, None, None);
    }
    let (page, token) = store.list_tasks_filtered(None, 10, None);
    assert_eq!(page.len(), 3);
    assert!(token.is_none(), "No more pages when results <= limit");
}

// ---------------------------------------------------------------------------
// Subscriptions
// ---------------------------------------------------------------------------

#[test]
fn test_subscribe_receives_status_update() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let store = InMemoryTaskStore::new();
        let task = store.create_task(None, None, None, None);
        let (mut rx, _) = store.subscribe(&task.id);

        store
            .update_status(&task.id, TaskState::Working, None)
            .unwrap();

        let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await;
        assert!(event.is_ok(), "Should receive status update");
        match event.unwrap().unwrap() {
            TaskEvent::StatusChanged { status, .. } => {
                assert_eq!(status.state, TaskState::Working);
            }
            _ => panic!("expected StatusChanged"),
        }
    });
}

#[test]
fn test_subscribe_receives_artifact_added() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let store = InMemoryTaskStore::new();
        let task = store.create_task(None, None, None, None);
        let (mut rx, _) = store.subscribe(&task.id);

        let artifact = Artifact {
            artifact_id: "art-1".to_string(),
            name: Some("result".to_string()),
            parts: vec![Part::Text {
                text: "output".into(),
            }],
            metadata: None,
        };
        store.add_artifact(&task.id, artifact.clone()).unwrap();

        let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await;
        assert!(event.is_ok(), "Should receive artifact added event");
        match event.unwrap().unwrap() {
            TaskEvent::ArtifactAdded { artifact: a, .. } => {
                assert_eq!(a.artifact_id, "art-1");
                assert_eq!(a.name, Some("result".to_string()));
            }
            _ => panic!("expected ArtifactAdded"),
        }
    });
}

// ---------------------------------------------------------------------------
// Push notification configs
// ---------------------------------------------------------------------------

#[test]
fn test_create_and_list_push_config() {
    let store = InMemoryTaskStore::new();
    let task = store.create_task(None, None, None, None);
    let config = store.create_push_config(&task.id, "https://hook.example.com/notify", None);
    assert_eq!(config.url, "https://hook.example.com/notify");
    assert_eq!(config.task_id, task.id);
    assert!(!config.id.is_empty());

    let configs = store.list_push_configs(&task.id);
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].url, "https://hook.example.com/notify");
}

#[test]
fn test_create_push_config_with_bearer_auth() {
    let store = InMemoryTaskStore::new();
    let task = store.create_task(None, None, None, None);
    let auth = PushAuthenticationInfo {
        scheme: PushAuthScheme::Bearer {
            token: "sekret".to_string(),
        },
    };
    let config = store.create_push_config(&task.id, "https://hook.example.com/notify", Some(auth));
    assert!(config.authentication.is_some());
    let auth_scheme = &config.authentication.unwrap().scheme;
    match auth_scheme {
        PushAuthScheme::Bearer { token } => assert_eq!(token, "sekret"),
        _ => panic!("expected Bearer auth"),
    }
}

#[test]
fn test_delete_push_config() {
    let store = InMemoryTaskStore::new();
    let task = store.create_task(None, None, None, None);
    let config = store.create_push_config(&task.id, "https://hook.example.com/notify", None);
    assert!(!store.list_push_configs(&task.id).is_empty());

    store.delete_push_config(&task.id, &config.id);
    assert!(store.list_push_configs(&task.id).is_empty());
}

#[test]
fn test_get_push_config() {
    let store = InMemoryTaskStore::new();
    let task = store.create_task(None, None, None, None);
    let config = store.create_push_config(&task.id, "https://hook.example.com/notify", None);

    let found = store.get_push_config(&task.id, &config.id);
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, config.id);

    let not_found = store.get_push_config(&task.id, "nonexistent");
    assert!(not_found.is_none());
}

// ---------------------------------------------------------------------------
// Trait contract — verify all TaskStoreBackend methods compile and execute
// through a trait object reference.
// ---------------------------------------------------------------------------

#[test]
fn test_trait_contract_immemory() {
    let store: Box<dyn TaskStoreBackend> = Box::new(InMemoryTaskStore::new());

    // create_task
    let task = store.create_task(None, None, None, None);
    assert!(!task.id.is_empty(), "Task should have an ID");
    assert_eq!(task.status.state, TaskState::Submitted);

    // get_task
    let retrieved = store.get_task(&task.id).unwrap();
    assert_eq!(retrieved.id, task.id);

    // update_status
    let updated = store
        .update_status(&task.id, TaskState::Working, None)
        .unwrap();
    assert_eq!(updated.status.state, TaskState::Working);

    // add_artifact
    let artifact = Artifact {
        artifact_id: "art-1".to_string(),
        name: Some("test".to_string()),
        parts: vec![],
        metadata: None,
    };
    let with_artifact = store.add_artifact(&task.id, artifact).unwrap();
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
        .create_task_with_idempotency("trait-key-1", None, None, None, None)
        .unwrap();
    assert!(!new_task.id.is_empty());

    // create_task_with_idempotency — duplicate key
    let dup = store.create_task_with_idempotency("trait-key-1", None, None, None, None);
    assert!(dup.is_err(), "duplicate key should return Err");
    let boxed = dup.unwrap_err();
    let (existing_task, _sender) = *boxed;
    assert_eq!(
        existing_task.id, new_task.id,
        "duplicate should return existing task"
    );
}
