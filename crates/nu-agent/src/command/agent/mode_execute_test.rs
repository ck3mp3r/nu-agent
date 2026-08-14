use super::mode_execute;
use nu_agent_a2a::{InMemoryTaskStore, TaskState};
use nu_protocol::{Value, record};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// auto_complete_a2a_task tests
// ---------------------------------------------------------------------------

/// Helper: create a task in Working state (the only state that can transition to Completed).
fn setup_working_task(store: &InMemoryTaskStore) -> String {
    let task = store.create_task(None, None, None, None);
    let task_id = task.id.clone();
    store
        .update_status(&task_id, TaskState::Working, None)
        .unwrap();
    task_id
}

#[test]
fn auto_complete_with_string_response_completes_task() {
    let store = Arc::new(InMemoryTaskStore::new());
    let task_id = setup_working_task(&store);

    let response = Value::test_string("The answer is 42.");
    mode_execute::auto_complete_a2a_task(&store, &task_id, &response);

    let task = store.get_task(&task_id).unwrap();
    assert_eq!(task.status.state, TaskState::Completed);
}

#[test]
fn auto_complete_with_record_and_response_field_completes_task() {
    let store = Arc::new(InMemoryTaskStore::new());
    let task_id = setup_working_task(&store);

    let response = Value::test_record(record! {
        "response" => Value::test_string("Hello from record!"),
    });
    mode_execute::auto_complete_a2a_task(&store, &task_id, &response);

    let task = store.get_task(&task_id).unwrap();
    assert_eq!(task.status.state, TaskState::Completed);
}

#[test]
fn auto_complete_with_record_missing_response_field_uses_fallback() {
    let store = Arc::new(InMemoryTaskStore::new());
    let task_id = setup_working_task(&store);

    let response = Value::test_record(record! {
        "foo" => Value::test_string("bar"),
    });
    mode_execute::auto_complete_a2a_task(&store, &task_id, &response);

    let task = store.get_task(&task_id).unwrap();
    assert_eq!(task.status.state, TaskState::Completed);
    // The fallback text should result in a non-empty artifact.
    assert!(!task.artifacts.is_empty(), "should have result artifact");
}

#[test]
fn auto_complete_with_non_text_handles_gracefully() {
    let store = Arc::new(InMemoryTaskStore::new());
    let task_id = setup_working_task(&store);

    // Integer values have no "response" field, so it uses the fallback.
    let response = Value::test_int(42);
    mode_execute::auto_complete_a2a_task(&store, &task_id, &response);

    let task = store.get_task(&task_id).unwrap();
    assert_eq!(task.status.state, TaskState::Completed);
}

#[test]
fn auto_complete_with_record_and_response_field_uses_correct_text() {
    let store = Arc::new(InMemoryTaskStore::new());
    let task_id = setup_working_task(&store);

    let response = Value::test_record(record! {
        "response" => Value::test_string("Exact match"),
    });
    mode_execute::auto_complete_a2a_task(&store, &task_id, &response);

    let task = store.get_task(&task_id).unwrap();
    // Inspect the result artifact for the expected text.
    let artifacts = &task.artifacts;
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].name.as_deref(), Some("result"));
}
