use super::*;

use crate::conversation::turn::{TurnEnd, TurnFailure};

// ---------------------------------------------------------------------------
// Session stage — external task outcome publication
// ---------------------------------------------------------------------------

fn take_task_events(lifecycle_rx: &mut LifecycleRx) -> Vec<LifecycleEvent> {
    let mut out = Vec::new();
    while let Ok(event) = lifecycle_rx.try_recv() {
        out.push(event);
    }
    out
}

#[tokio::test]
async fn external_turn_error_publishes_task_failed() -> Result<()> {
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

    // -- Check
    let events = take_task_events(lifecycle_rx);
    assert_eq!(
        events.len(),
        1,
        "exactly one turn event should be published, got {events:?}"
    );
    match &events[0] {
        LifecycleEvent::TaskFailed { task_id, error } => {
            assert_eq!(task_id, "task-1");
            assert_eq!(error, "provider exploded");
        }
        other => return Err(format!("expected TaskFailed, got {other:?}").into()),
    }
    Ok(())
}

#[tokio::test]
async fn external_turn_cancelled_publishes_task_failed() -> Result<()> {
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
            TurnResult::from_end(TurnEnd::Cancelled {
                value: Value::nothing(Span::test_data()),
            })
            .with_task_id(Some("task-2".to_string())),
            &mut ctx,
        )
        .await;

    // -- Check
    let events = take_task_events(lifecycle_rx);
    assert_eq!(
        events.len(),
        1,
        "exactly one turn event should be published, got {events:?}"
    );
    match &events[0] {
        LifecycleEvent::TaskFailed { task_id, error } => {
            assert_eq!(task_id, "task-2");
            assert_eq!(
                error,
                crate::orchestrator::stages::session::TASK_CANCELLED_NOTICE
            );
        }
        other => return Err(format!("expected TaskFailed, got {other:?}").into()),
    }
    Ok(())
}

#[tokio::test]
async fn external_turn_success_publishes_task_completed() -> Result<()> {
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
            TurnResult::from_end(TurnEnd::Completed {
                value: Value::string("done", Span::test_data()),
            })
            .with_task_id(Some("task-3".to_string())),
            &mut ctx,
        )
        .await;

    // -- Check
    let events = take_task_events(lifecycle_rx);
    assert_eq!(
        events.len(),
        1,
        "exactly one turn event should be published, got {events:?}"
    );
    match &events[0] {
        LifecycleEvent::TaskCompleted { task_id, .. } => assert_eq!(task_id, "task-3"),
        other => return Err(format!("expected TaskCompleted, got {other:?}").into()),
    }
    Ok(())
}

#[tokio::test]
async fn non_external_turn_error_publishes_no_task_failed() -> Result<()> {
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
                "boom",
            ))))
            .with_task_id(None),
            &mut ctx,
        )
        .await;

    // -- Check
    let events = take_task_events(lifecycle_rx);
    assert!(
        events.is_empty(),
        "a user turn failure must not publish a task event, got {events:?}"
    );
    Ok(())
}
