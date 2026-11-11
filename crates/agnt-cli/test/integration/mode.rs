//! Integration tests for the `mode` module's turn→terminal mapping.
//!
//! The success paths are covered by the unit tests; these drive the store error
//! branches, which log a warning and never propagate.

use agnt_a2a::InMemoryTaskStore;
use agnt_runtime::bus::{Bus, LifecycleEvent};
use nu_protocol::{Span, Value};

// region:    --- Tests

#[test]
fn test_integration_mode_apply_turn_terminal_unknown_task_is_non_fatal() {
    // -- Setup & Fixtures
    let store = InMemoryTaskStore::default();
    let bus = Bus::default();
    let ok: crate::error::Result<Value> = Ok(Value::string("done", Span::unknown()));
    let err: crate::error::Result<Value> = Err(crate::error::Error::Custom("boom".to_string()));

    // -- Exec
    crate::mode::apply_turn_terminal(
        &store,
        bus.lifecycle(),
        "missing",
        crate::mode::TaskTerminal::from_result(&ok),
    );
    crate::mode::apply_turn_terminal(
        &store,
        bus.lifecycle(),
        "missing",
        crate::mode::TaskTerminal::from_result(&err),
    );

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
        &LifecycleEvent::TaskCompleted {
            output: "ok".to_string(),
            task_id: "missing".to_string(),
        },
    );
    crate::mode::apply_turn_event_to_store(
        &store,
        &LifecycleEvent::TaskFailed {
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
