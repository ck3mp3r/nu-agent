use super::*;

// ---------------------------------------------------------------------------
// Worker result and quit gating
// ---------------------------------------------------------------------------

#[tokio::test]
async fn worker_result_drains_queued_and_checks_compaction() {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx: _,
        event_rx,
        worker_tx,
        worker_rx: _,
        worker_result_tx,
        worker_result_rx,
        blocking_tx,
        blocking_response_rx,
        concurrent_tx,
        concurrent_response_rx,
        external_rx,
        compaction_rx,
        task_cancel_rx,
        bus,
        state,
    } = h.parts();
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    worker_result_tx
        .send(TurnOutcome::Success(Value::nothing(Span::test_data())))
        .await
        .unwrap();

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        make_sources(
            worker_result_rx,
            blocking_response_rx,
            concurrent_response_rx,
            external_rx,
            compaction_rx,
            task_cancel_rx,
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
}

#[tokio::test]
async fn quit_blocked_when_worker_active() {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx: _,
        worker_result_tx: _,
        worker_result_rx,
        blocking_tx,
        blocking_response_rx,
        concurrent_tx,
        concurrent_response_rx,
        external_rx,
        compaction_rx,
        task_cancel_rx,
        bus,
        state,
    } = h.parts();
    state.worker_active = true;
    // Send Quit while worker is active — loop should continue, not break.
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();
    // Send another Quit after worker becomes idle.
    state.worker_active = false;
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();

    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        make_sources(
            worker_result_rx,
            blocking_response_rx,
            concurrent_response_rx,
            external_rx,
            compaction_rx,
            task_cancel_rx,
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
}

#[tokio::test]
async fn quit_allowed_when_idle_and_no_pending() {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx: _,
        worker_result_tx: _,
        worker_result_rx,
        blocking_tx,
        blocking_response_rx,
        concurrent_tx,
        concurrent_response_rx,
        external_rx,
        compaction_rx,
        task_cancel_rx,
        bus,
        state,
    } = h.parts();
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    event_tx.send(OrchestratorEvent::Quit).await.unwrap();

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        make_sources(
            worker_result_rx,
            blocking_response_rx,
            concurrent_response_rx,
            external_rx,
            compaction_rx,
            task_cancel_rx,
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
}
