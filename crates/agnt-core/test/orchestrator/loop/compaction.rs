use super::*;

// ---------------------------------------------------------------------------
// Compaction dispatch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn run_compaction_dispatches_when_worker_idle() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx: _,
        event_rx,
        worker_tx,
        worker_rx,
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

    bus.compaction()
        .send(CompactionEvent::Requested {
            source: "auto".to_string(),
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
    let cmd = recv_command(worker_rx).ok_or("RunCompaction should be dispatched")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => {
            assert_eq!(source, "auto");
        }
        _ => panic!("expected RunCompaction"),
    }
    assert!(
        !state.worker_active,
        "worker_active must NOT be set for a compaction command (the worker emits events on the bus)"
    );
    Ok(())
}

#[tokio::test]
async fn run_compaction_queued_when_worker_busy_then_dispatched_on_idle() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx: _,
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
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    // Worker is busy running a turn.
    state.worker_active = true;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    bus.compaction()
        .send(CompactionEvent::Requested {
            source: "auto".to_string(),
        })
        .await
        .unwrap();
    // Worker finishes; the queued compaction must be dispatched.
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
    let cmd = recv_command(worker_rx).ok_or("queued RunCompaction should be dispatched")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => {
            assert_eq!(source, "auto");
        }
        _ => panic!("expected RunCompaction"),
    }
    Ok(())
}

#[tokio::test]
async fn run_compaction_queued_replaces_previous_when_busy() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx: _,
        event_rx,
        worker_tx,
        worker_rx,
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

    // First request dispatches immediately and marks a compaction active.
    bus.compaction()
        .send(CompactionEvent::Requested {
            source: "auto".to_string(),
        })
        .await
        .unwrap();
    // Second request queued while a compaction is active.
    bus.compaction()
        .send(CompactionEvent::Requested {
            source: "slash".to_string(),
        })
        .await
        .unwrap();
    // Third request replaces the queued one.
    bus.compaction()
        .send(CompactionEvent::Requested {
            source: "manual".to_string(),
        })
        .await
        .unwrap();
    // The active compaction completes; the newest queued request is dispatched.
    bus.compaction()
        .send(CompactionEvent::Completed {
            source: "auto".to_string(),
            summary_preview: String::new(),
            summary_body: String::new(),
        })
        .await
        .unwrap();
    // The dispatched queued compaction completes too, so the loop can exit.
    bus.compaction()
        .send(CompactionEvent::Completed {
            source: "manual".to_string(),
            summary_preview: String::new(),
            summary_body: String::new(),
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
    let cmd = recv_command(worker_rx).ok_or("first RunCompaction should be dispatched")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => assert_eq!(source, "auto"),
        _ => panic!("expected RunCompaction"),
    }
    let cmd = recv_command(worker_rx).ok_or("queued RunCompaction should be dispatched")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => {
            assert_eq!(source, "manual", "newest queued request must win");
        }
        _ => panic!("expected RunCompaction"),
    }
    Ok(())
}

#[tokio::test]
async fn compaction_requested_while_active_is_queued() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx: _,
        event_rx,
        worker_tx,
        worker_rx,
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

    // First request dispatches immediately (worker idle, no compaction active).
    bus.compaction()
        .send(CompactionEvent::Requested {
            source: "auto".to_string(),
        })
        .await
        .unwrap();
    // Second request arrives while a compaction is active — it must be queued,
    // not dispatched.
    bus.compaction()
        .send(CompactionEvent::Requested {
            source: "slash".to_string(),
        })
        .await
        .unwrap();
    // The active compaction completes; the queued request is dispatched.
    bus.compaction()
        .send(CompactionEvent::Completed {
            source: "auto".to_string(),
            summary_preview: String::new(),
            summary_body: String::new(),
        })
        .await
        .unwrap();
    // The dispatched queued compaction completes too, so the loop can exit.
    bus.compaction()
        .send(CompactionEvent::Completed {
            source: "slash".to_string(),
            summary_preview: String::new(),
            summary_body: String::new(),
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
    // First command: the initial request.
    let cmd = recv_command(worker_rx).ok_or("first RunCompaction should be dispatched")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => assert_eq!(source, "auto"),
        _ => panic!("expected RunCompaction"),
    }
    // Second command: the queued request dispatched after completion.
    let cmd = recv_command(worker_rx).ok_or("queued RunCompaction should be dispatched")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => assert_eq!(source, "slash"),
        _ => panic!("expected RunCompaction"),
    }
    // No third command should be dispatched.
    assert!(
        recv_command(worker_rx).is_none(),
        "no further RunCompaction should be dispatched"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Quit gating around compaction
// ---------------------------------------------------------------------------

#[tokio::test]
async fn quit_waits_while_compaction_active() -> Result<()> {
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
        worker_result_tx: _,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut external_rx,
        mut compaction_rx,
        task_cancel_rx: _,
        bus,
        mut ctx_state,
    } = h;

    // Run the loop concurrently. Pass `None` for the task-cancel source so the
    // loop does not break on a closed cancel channel; it can only exit via Quit.
    let bus_driver = bus.clone();
    let loop_task = tokio::spawn(async move {
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
        let mut task_cancel_rx: Option<mpsc::UnboundedReceiver<String>> = None;
        run_orchestrator_loop(
            event_rx,
            make_sources(
                &mut worker_result_rx,
                &mut blocking_response_rx,
                &mut concurrent_response_rx,
                &mut external_rx,
                &mut compaction_rx,
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
        .await
    });

    // Start a compaction so it is active when Quit arrives.
    bus_driver
        .compaction()
        .send(CompactionEvent::Requested {
            source: "auto".to_string(),
        })
        .await
        .unwrap();
    // Wait until the compaction command is dispatched (compaction is active).
    let cmd = worker_rx.as_mut().ok_or("worker_rx present")?.recv();
    let cmd = tokio::time::timeout(std::time::Duration::from_secs(5), cmd)
        .await
        .map_err(|_| "RunCompaction should be dispatched")?
        .ok_or("worker channel should not close")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => assert_eq!(source, "auto"),
        _ => panic!("expected RunCompaction"),
    }

    // Request quit while the compaction is still active.
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();
    // Give the loop a chance to process Quit; it must NOT exit yet.
    tokio::task::yield_now().await;
    assert!(
        !loop_task.is_finished(),
        "loop must not exit while a compaction is active"
    );

    // The compaction completes; the loop may now exit.
    bus_driver
        .compaction()
        .send(CompactionEvent::Completed {
            source: "auto".to_string(),
            summary_preview: String::new(),
            summary_body: String::new(),
        })
        .await
        .unwrap();

    let result = loop_task
        .await
        .map_err(|e| format!("loop task should not panic: {e}"))?;
    assert!(result.is_ok());
    Ok(())
}

#[tokio::test]
async fn quit_exits_after_compaction_completes() -> Result<()> {
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
        worker_result_tx: _,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut external_rx,
        mut compaction_rx,
        task_cancel_rx: _,
        bus,
        mut ctx_state,
    } = h;

    let bus_driver = bus.clone();
    let loop_task = tokio::spawn(async move {
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
        let mut task_cancel_rx: Option<mpsc::UnboundedReceiver<String>> = None;
        run_orchestrator_loop(
            event_rx,
            make_sources(
                &mut worker_result_rx,
                &mut blocking_response_rx,
                &mut concurrent_response_rx,
                &mut external_rx,
                &mut compaction_rx,
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
        .await
    });

    // Start a compaction so it is active when Quit arrives.
    bus_driver
        .compaction()
        .send(CompactionEvent::Requested {
            source: "auto".to_string(),
        })
        .await
        .unwrap();
    // Wait until the compaction command is dispatched (compaction is active).
    let cmd = worker_rx.as_mut().ok_or("worker_rx present")?.recv();
    let cmd = tokio::time::timeout(std::time::Duration::from_secs(5), cmd)
        .await
        .map_err(|_| "RunCompaction should be dispatched")?
        .ok_or("worker channel should not close")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => assert_eq!(source, "auto"),
        _ => panic!("expected RunCompaction"),
    }

    // Request quit while the compaction is active, then complete the compaction.
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();
    bus_driver
        .compaction()
        .send(CompactionEvent::Completed {
            source: "auto".to_string(),
            summary_preview: String::new(),
            summary_body: String::new(),
        })
        .await
        .unwrap();

    let result = loop_task
        .await
        .map_err(|e| format!("loop task should not panic: {e}"))?;
    assert!(result.is_ok());
    Ok(())
}
