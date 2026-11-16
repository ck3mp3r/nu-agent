//! The session stage publishes the spine's turn-end step on a failed turn.
//!
//! The input lock releases on `LifecycleEvent::TurnCompleted`, which rides the
//! strict spine and never drops. The executor publishes that step on a
//! successful turn and on a cancelled one, and returns before both sites on a
//! failure — so the stage, the owner of the failure's other lifecycle rows, is
//! where the failed turn's release is published.

use super::*;

use crate::conversation::turn::{TurnEnd, TurnFailure};

/// The one spine step a turn end publishes, with the release's synthesized
/// duration.
fn spine_step(task_id: Option<String>) -> LifecycleEvent {
    LifecycleEvent::TurnCompleted {
        task_id,
        duration_ms: 0,
    }
}

#[tokio::test]
async fn failed_turn_publishes_spine_turn_completed() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = Harness::new();
    let HarnessParts {
        stage: _,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx: _,
        narration_rx: _,
        lifecycle_rx,
        stream_rx: _,
        state,
    } = h.parts();
    let mut session = SessionStage;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    // -- Exec: a user turn carries no task id.
    session
        .handle_outcome(
            TurnResult::from_end(TurnEnd::Failed(TurnFailure::from_error(LabeledError::new(
                "provider exploded",
            ))))
            .with_task_id(None),
            &mut ctx,
        )
        .await;

    // -- Check: the spine step is the only lifecycle row a user turn publishes,
    // and it is the row the input lock releases on.
    let events = take_lifecycle(lifecycle_rx);
    assert_eq!(
        events,
        vec![spine_step(None)],
        "a failed turn must publish exactly one spine step, got {events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn failed_external_turn_publishes_spine_beside_task_failed() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = Harness::new();
    let HarnessParts {
        stage: _,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx: _,
        narration_rx: _,
        lifecycle_rx,
        stream_rx: _,
        state,
    } = h.parts();
    let mut session = SessionStage;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    // -- Exec: the result carries the task the turn ran for.
    session
        .handle_outcome(
            TurnResult::from_end(TurnEnd::Failed(TurnFailure::from_error(LabeledError::new(
                "provider exploded",
            ))))
            .with_task_id(Some("task-1".to_string())),
            &mut ctx,
        )
        .await;

    // -- Check: the external task terminal and the turn's own spine step.
    let events = take_lifecycle(lifecycle_rx);
    assert_eq!(
        events,
        vec![
            LifecycleEvent::TaskFailed {
                task_id: "task-1".to_string(),
                error: "provider exploded".to_string(),
            },
            spine_step(Some("task-1".to_string())),
        ],
        "a failed external turn publishes its task row and one spine step, got {events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn successful_turn_publishes_no_spine_step() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = Harness::new();
    let HarnessParts {
        stage: _,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx: _,
        narration_rx: _,
        lifecycle_rx,
        stream_rx: _,
        state,
    } = h.parts();
    let mut session = SessionStage;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    // -- Exec
    session
        .handle_outcome(
            TurnResult::from_end(TurnEnd::Completed {
                value: Value::string("done", Span::test_data()),
            })
            .with_task_id(Some("task-3".to_string())),
            &mut ctx,
        )
        .await;

    // -- Check: the executor owns the successful turn's spine step, so the
    // stage adds none.
    let events = take_lifecycle(lifecycle_rx);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, LifecycleEvent::TurnCompleted { .. })),
        "a successful turn must not get a second spine step from the stage, got {events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn cancelled_turn_publishes_no_spine_step() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = Harness::new();
    let HarnessParts {
        stage: _,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx: _,
        narration_rx: _,
        lifecycle_rx,
        stream_rx: _,
        state,
    } = h.parts();
    let mut session = SessionStage;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    // -- Exec
    session
        .handle_outcome(
            TurnResult::from_end(TurnEnd::Cancelled {
                value: Value::nothing(Span::test_data()),
            })
            .with_task_id(Some("task-2".to_string())),
            &mut ctx,
        )
        .await;

    // -- Check: the executor publishes the cancelled turn's spine step at its
    // early return, so the stage adds none.
    let events = take_lifecycle(lifecycle_rx);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, LifecycleEvent::TurnCompleted { .. })),
        "a cancelled turn must not get a second spine step from the stage, got {events:?}"
    );
    Ok(())
}
