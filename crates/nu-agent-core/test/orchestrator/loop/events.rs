use super::*;

// ---------------------------------------------------------------------------
// Prompt routing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prompt_submitted_routes_to_slash() {
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

    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: "hello".to_string(),
        })
        .await
        .unwrap();
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
    assert_eq!(slash.handle_calls, vec!["hello".to_string()]);
}

#[tokio::test]
async fn prompt_submitted_blocked_when_blocking_pending() {
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
    ui_request.blocking_pending = true;
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: "hello".to_string(),
        })
        .await
        .unwrap();
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
    assert!(
        slash.handle_calls.is_empty(),
        "slash.handle should NOT be called"
    );
}

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

// ---------------------------------------------------------------------------
// External prompt and fatal error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn external_prompt_dispatches_turn_when_idle() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx,
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
    let mut slash = SlashStage;
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    let mut turn_rx = bus.turn().subscribe();

    // The external prompt is enqueued on the UI bus; the TUI answers with a
    // `PromptSubmitted` once the prompt reaches the front of its queue.
    bus.external()
        .send(ExternalEvent::PromptReceived {
            prompt: "external task".to_string(),
            task_id: "task-1".to_string(),
            context_id: None,
        })
        .await
        .unwrap();
    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: "external task".to_string(),
        })
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
    assert!(
        state.worker_active,
        "worker_active should be true after the turn is dispatched"
    );
    assert_eq!(
        state.active_external_prompt.as_deref(),
        Some("external task")
    );
    assert_eq!(state.active_external_task_id.as_deref(), Some("task-1"));
    assert_eq!(
        state.pending_a2a_task_id, None,
        "the pending task id is consumed by the turn dispatch"
    );
    let cmd = recv_command(worker_rx).ok_or("ExecuteTurn should be dispatched")?;
    match cmd {
        WorkerCommand::ExecuteTurn { prompt, .. } => {
            assert_eq!(prompt, "external task");
        }
        _ => panic!("expected ExecuteTurn"),
    }
    let started = turn_rx
        .try_recv()
        .map_err(|e| format!("TurnEvent::Started should be published: {e:?}"))?;
    match started {
        TurnEvent::Started { task_id, .. } => {
            assert_eq!(
                task_id.as_deref(),
                Some("task-1"),
                "the A2A turn-start event must name task-1"
            );
        }
        _ => panic!("expected TurnEvent::Started"),
    }
    let _ = worker_result_tx;
    Ok(())
}

#[tokio::test]
async fn fatal_error_returns_err() -> Result<()> {
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

    event_tx
        .send(OrchestratorEvent::FatalError(LabeledError::new("fatal")))
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

    let Err(err) = result else {
        return Err("run_orchestrator_loop should fail with fatal error".into());
    };
    assert_eq!(err.msg, "fatal");
    Ok(())
}
