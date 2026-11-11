use agnt_kernel::CompactionSource;

use super::*;

use crate::conversation::turn::TurnEnd;

// ---------------------------------------------------------------------------
// Compaction dispatch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn run_compaction_dispatches_when_worker_idle() -> Result<()> {
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
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

    bus.control()
        .publish(crate::bus::ControlCommand::CompactionRequested {
            source: CompactionSource::Auto,
        })?;

    // The loop stops once the surface's intake closes: no cancel source is
    // configured, so the closed event channel is the conscious exit.
    let driver = async {
        let cmd = recv_worker_command(&mut worker_rx, "RunCompaction").await?;
        match cmd {
            WorkerCommand::RunCompaction { source } => {
                assert_eq!(source, CompactionSource::Auto);
            }
            _ => return Err("expected RunCompaction".into()),
        }
        drop(event_tx);
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let (result, driven) = tokio::join!(
        run_orchestrator_loop(
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
        ),
        driver
    );
    driven?;

    assert!(result.is_ok());
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "a compaction command is dispatched once"
    );
    assert!(
        !ctx_state.worker_active,
        "worker_active must NOT be set for a compaction command (the worker emits events on the bus)"
    );
    Ok(())
}

#[tokio::test]
async fn run_compaction_queued_when_worker_busy_then_dispatched_on_idle() -> Result<()> {
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
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
    // Worker is busy running a turn.
    ctx_state.worker_active = true;
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );

    bus.control()
        .publish(crate::bus::ControlCommand::CompactionRequested {
            source: CompactionSource::Auto,
        })?;
    // Worker finishes; the queued compaction must be dispatched.
    worker_result_tx
        .send(TurnResult::from_end(TurnEnd::Completed {
            value: Value::nothing(Span::test_data()),
        }))
        .await?;

    // No cancel source is configured, so the closed event channel is the loop's
    // exit; the worker's turn result is the event that frees the queued sweep.
    let driver = async {
        match recv_worker_command(&mut worker_rx, "queued RunCompaction").await? {
            WorkerCommand::RunCompaction { source } => {
                assert_eq!(source, CompactionSource::Auto);
            }
            _ => return Err("expected RunCompaction".into()),
        }
        drop(event_tx);
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let (result, driven) = tokio::join!(
        run_orchestrator_loop(
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
        ),
        driver
    );
    driven?;

    assert!(result.is_ok());
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "a queued sweep is dispatched once"
    );
    Ok(())
}

/// The pending-compaction slot holds one request: a newer request that arrives
/// while a compaction is in flight replaces the parked one, so the completion
/// dispatches the newest.
#[tokio::test]
async fn run_compaction_queued_replaces_previous_when_busy() -> Result<()> {
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
        mut control_rx,
        mut lifecycle_rx,
        mut task_cancel_rx,
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
        run_orchestrator_loop(
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
        .await
    });

    // First request dispatches immediately and makes a compaction active.
    bus_driver
        .control()
        .publish(crate::bus::ControlCommand::CompactionRequested {
            source: CompactionSource::Auto,
        })?;
    match recv_worker_command(&mut worker_rx, "first RunCompaction").await? {
        WorkerCommand::RunCompaction { source } => assert_eq!(source, CompactionSource::Auto),
        _ => panic!("expected RunCompaction"),
    }

    // A second request is parked while the compaction is active, and a third
    // replaces it in the slot.
    for source in [CompactionSource::Manual, CompactionSource::Auto] {
        bus_driver
            .control()
            .publish(crate::bus::ControlCommand::CompactionRequested { source })?;
    }
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "a request that arrives while a compaction is active must be queued, not dispatched"
    );

    // The active compaction completes; the newest parked request is dispatched.
    bus_driver
        .lifecycle()
        .publish(crate::bus::LifecycleEvent::CompactionCompleted {
            source: CompactionSource::Auto,
            summary: String::new(),
        })?;
    match recv_worker_command(&mut worker_rx, "queued RunCompaction").await? {
        WorkerCommand::RunCompaction { source } => {
            assert_eq!(
                source,
                CompactionSource::Auto,
                "newest queued request must win"
            );
        }
        _ => panic!("expected RunCompaction"),
    }

    // The dispatched queued compaction completes too, so the loop can exit.
    bus_driver
        .lifecycle()
        .publish(crate::bus::LifecycleEvent::CompactionCompleted {
            source: CompactionSource::Auto,
            summary: String::new(),
        })?;
    drop(event_tx);
    let result = loop_task
        .await
        .map_err(|e| format!("loop task should not panic: {e}"))?;
    assert!(result.is_ok());
    Ok(())
}

/// A request that arrives while a compaction is in flight is parked and
/// dispatched when that compaction completes.
///
/// The loop runs concurrently so each publication is observed before the next
/// one: `control` and `lifecycle` are separate channels, so a queue pre-loaded
/// with both would be consumed in the loop's poll order, not in publication
/// order.
#[tokio::test]
async fn compaction_requested_while_active_is_queued() -> Result<()> {
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
        mut control_rx,
        mut lifecycle_rx,
        mut task_cancel_rx,
        bus,
        mut ctx_state,
    } = h;

    // Run the loop concurrently. With no cancel source that arm stays pending,
    // so the loop can only exit when the event channel closes.
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
        run_orchestrator_loop(
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
        .await
    });

    // First request dispatches immediately: the worker is idle and no
    // compaction is active.
    bus_driver
        .control()
        .publish(crate::bus::ControlCommand::CompactionRequested {
            source: CompactionSource::Auto,
        })?;
    match recv_worker_command(&mut worker_rx, "first RunCompaction").await? {
        WorkerCommand::RunCompaction { source } => assert_eq!(source, CompactionSource::Auto),
        _ => panic!("expected RunCompaction"),
    }

    // Second request arrives while the compaction is active: it is parked in the
    // pending-compaction slot, not dispatched.
    bus_driver
        .control()
        .publish(crate::bus::ControlCommand::CompactionRequested {
            source: CompactionSource::Manual,
        })?;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "a request that arrives while a compaction is active must be queued, not dispatched"
    );

    // The active compaction completes; the parked request is dispatched.
    bus_driver
        .lifecycle()
        .publish(crate::bus::LifecycleEvent::CompactionCompleted {
            source: CompactionSource::Auto,
            summary: String::new(),
        })?;
    match recv_worker_command(&mut worker_rx, "queued RunCompaction").await? {
        WorkerCommand::RunCompaction { source } => assert_eq!(source, CompactionSource::Manual),
        _ => panic!("expected RunCompaction"),
    }

    // That compaction completes too; nothing further is dispatched.
    bus_driver
        .lifecycle()
        .publish(crate::bus::LifecycleEvent::CompactionCompleted {
            source: CompactionSource::Manual,
            summary: String::new(),
        })?;
    drop(event_tx);
    let result = loop_task
        .await
        .map_err(|e| format!("loop task should not panic: {e}"))?;
    assert!(result.is_ok());
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "no further RunCompaction should be dispatched"
    );
    Ok(())
}
