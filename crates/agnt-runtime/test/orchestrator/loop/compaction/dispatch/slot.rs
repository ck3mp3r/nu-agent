//! The dispatch holds no slot until the pipeline says a sweep began.

use super::*;

/// WHEN a rewrite is dispatched and the pipeline publishes no start row, THEN
/// THE loop SHALL hold no slot for it: the next request dispatches instead of
/// parking, and quit does not wait on it.
#[tokio::test]
async fn a_dispatch_without_a_start_row_holds_no_slot() -> Result<()> {
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

    // -- Exec: two requests with no start row between them. A dispatch is not a
    // started sweep, so the second one dispatches too.
    for (source, what) in [
        (CompactionSource::Auto, "first RunCompaction"),
        (CompactionSource::Manual, "second RunCompaction"),
    ] {
        bus_driver
            .control()
            .publish(crate::bus::ControlCommand::CompactionRequested { source })?;
        match recv_worker_command(&mut worker_rx, what).await? {
            WorkerCommand::RunCompaction { source: asked } => assert_eq!(asked, source),
            _ => return Err(format!("{what} should be a RunCompaction").into()),
        }
    }

    // -- Exec & Check: nothing is in flight, so quit owes nothing and exits.
    event_tx.send(OrchestratorEvent::Quit).await?;
    let result = loop_task
        .await
        .map_err(|e| format!("loop task should not panic: {e}"))?;
    assert!(
        result.is_ok(),
        "a dispatch that started no sweep holds no slot to wait for"
    );
    Ok(())
}
