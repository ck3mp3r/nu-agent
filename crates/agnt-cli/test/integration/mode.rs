//! Integration tests for the `mode` module's A2A task-store helpers.
//!
//! The success paths are covered by the unit tests; these drive the store
//! error branches, which log a warning and never propagate.

use agnt_a2a::InMemoryTaskStore;
use agnt_core::bus::TurnEvent;
use nu_protocol::{Span, Value};

// region:    --- Tests

#[test]
fn test_integration_mode_auto_complete_unknown_task_is_non_fatal() {
    // -- Setup & Fixtures
    let store = InMemoryTaskStore::default();

    // -- Exec
    crate::mode::auto_complete_a2a_task(&store, "missing", &Value::string("done", Span::unknown()));

    // -- Check
    assert!(
        store.get_task("missing").is_err(),
        "an unknown task must stay absent"
    );
}

#[test]
fn test_integration_mode_auto_fail_unknown_task_is_non_fatal() {
    // -- Setup & Fixtures
    let store = InMemoryTaskStore::default();

    // -- Exec
    crate::mode::auto_fail_a2a_task(&store, "missing", "boom");

    // -- Check
    assert!(
        store.get_task("missing").is_err(),
        "an unknown task must stay absent"
    );
}

#[test]
fn test_integration_mode_apply_turn_event_unknown_task_is_non_fatal() {
    // -- Setup & Fixtures
    let store = InMemoryTaskStore::default();

    // -- Exec
    crate::mode::apply_turn_event_to_store(
        &store,
        &TurnEvent::TaskCompleted {
            output: "ok".to_string(),
            task_id: "missing".to_string(),
        },
    );
    crate::mode::apply_turn_event_to_store(
        &store,
        &TurnEvent::TaskFailed {
            task_id: "missing".to_string(),
            error: "boom".to_string(),
        },
    );

    // -- Check
    assert!(
        store.get_task("missing").is_err(),
        "an unknown task must stay absent"
    );
}

// endregion: --- Tests
