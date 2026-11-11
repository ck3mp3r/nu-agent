use super::*;

use crate::conversation::turn::TurnEnd;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Worker result and quit gating
// ---------------------------------------------------------------------------

#[tokio::test]
async fn worker_result_drains_queued_and_checks_compaction() -> Result<()> {
    let h = Harness::new();
    let Harness {
        event_rx,
        worker_tx,
        worker_result_tx,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut control_rx,
        mut lifecycle_rx,
        mut task_cancel_rx,
        bus,
        mut ctx_state,
        ..
    } = h;
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );

    worker_result_tx
        .send(TurnResult::from_end(TurnEnd::Completed {
            value: Value::nothing(Span::test_data()),
        }))
        .await?;

    // The worker-result source is the exit: the one buffered outcome drains on
    // the first pass, and the closed channel answers the next poll with `None` —
    // the same arm production breaks on when its worker task ends.
    drop(worker_result_tx);
    let result = run_orchestrator_loop(
        event_rx,
        make_sources(
            &mut worker_result_rx,
            &mut blocking_response_rx,
            &mut concurrent_response_rx,
            &mut control_rx,
            &mut lifecycle_rx,
            &mut task_cancel_rx,
        ),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(
        session.handle_outcome_calls.len(),
        1,
        "session.handle_outcome should be called"
    );
    assert_eq!(
        ui_request.drain_queued_calls, 1,
        "ui_request.drain_queued should be called"
    );
    Ok(())
}

#[tokio::test]
async fn quit_blocked_when_worker_active() -> Result<()> {
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut control_rx,
        mut lifecycle_rx,
        mut task_cancel_rx,
        bus,
        mut ctx_state,
        ..
    } = h;
    ctx_state.worker_active = true;
    // Send Quit while worker is active — loop should continue, not break.
    event_tx.send(OrchestratorEvent::Quit).await?;
    // Send another Quit after worker becomes idle.
    ctx_state.worker_active = false;
    event_tx.send(OrchestratorEvent::Quit).await?;

    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );

    let result = run_orchestrator_loop(
        event_rx,
        make_sources(
            &mut worker_result_rx,
            &mut blocking_response_rx,
            &mut concurrent_response_rx,
            &mut control_rx,
            &mut lifecycle_rx,
            &mut task_cancel_rx,
        ),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
    Ok(())
}

#[tokio::test]
async fn quit_allowed_when_idle_and_no_pending() -> Result<()> {
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut control_rx,
        mut lifecycle_rx,
        mut task_cancel_rx,
        bus,
        mut ctx_state,
        ..
    } = h;
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );

    event_tx.send(OrchestratorEvent::Quit).await?;

    let result = run_orchestrator_loop(
        event_rx,
        make_sources(
            &mut worker_result_rx,
            &mut blocking_response_rx,
            &mut concurrent_response_rx,
            &mut control_rx,
            &mut lifecycle_rx,
            &mut task_cancel_rx,
        ),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
    Ok(())
}
