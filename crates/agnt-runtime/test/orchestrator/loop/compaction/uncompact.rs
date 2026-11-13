//! Uncompaction dispatch through the orchestrator loop.
//!
//! The uncompaction travels the compaction spine: a control command to the
//! loop owner, a worker command, and a terminal spine row. It shares the
//! compaction-active slot, so one transcript rewrite runs at a time and a quit
//! is deferred for both.

use agnt_kernel::CompactionSource;

use super::*;

#[tokio::test]
async fn run_uncompact_dispatches_when_worker_idle() -> Result<()> {
    // -- Setup & Fixtures
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

    // -- Exec
    bus.control()
        .publish(crate::bus::ControlCommand::UncompactRequested {
            source: CompactionSource::Manual,
        })?;

    // The loop stops once the surface's intake closes: no cancel source is
    // configured, so the closed event channel is the conscious exit.
    let driver = async {
        match recv_worker_command(&mut worker_rx, "RunUncompact").await? {
            WorkerCommand::RunUncompact { source } => {
                assert_eq!(source, CompactionSource::Manual);
            }
            _ => return Err("expected RunUncompact".into()),
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

    // -- Check
    assert!(result.is_ok());
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "an uncompaction command is dispatched once"
    );
    assert!(
        !ctx_state.worker_active,
        "worker_active must NOT be set for an uncompaction command"
    );
    Ok(())
}

#[tokio::test]
async fn uncompact_requested_while_active_is_queued() -> Result<()> {
    // -- Setup & Fixtures
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

    // -- Exec: the first one dispatches and makes the slot active.
    bus_driver
        .control()
        .publish(crate::bus::ControlCommand::UncompactRequested {
            source: CompactionSource::Manual,
        })?;
    match recv_worker_command(&mut worker_rx, "first RunUncompact").await? {
        WorkerCommand::RunUncompact { source } => assert_eq!(source, CompactionSource::Manual),
        _ => panic!("expected RunUncompact"),
    }

    // -- Check: a second one is parked while the slot is active.
    bus_driver
        .control()
        .publish(crate::bus::ControlCommand::UncompactRequested {
            source: CompactionSource::Manual,
        })?;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "an uncompaction that arrives while one is active must be queued"
    );

    // -- Exec & Check: the terminal row dispatches the parked one.
    bus_driver
        .lifecycle()
        .publish(crate::bus::LifecycleEvent::Uncompacted {
            source: CompactionSource::Manual,
        })?;
    match recv_worker_command(&mut worker_rx, "queued RunUncompact").await? {
        WorkerCommand::RunUncompact { source } => assert_eq!(source, CompactionSource::Manual),
        _ => panic!("expected RunUncompact"),
    }

    // A refusal is a terminal row too, so the slot clears on it as well.
    bus_driver
        .lifecycle()
        .publish(crate::bus::LifecycleEvent::UncompactFailed {
            source: CompactionSource::Manual,
            message: "nothing to undo".to_string(),
        })?;
    drop(event_tx);
    let result = loop_task
        .await
        .map_err(|e| format!("loop task should not panic: {e}"))?;
    assert!(result.is_ok());
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "no further RunUncompact should be dispatched"
    );
    Ok(())
}
