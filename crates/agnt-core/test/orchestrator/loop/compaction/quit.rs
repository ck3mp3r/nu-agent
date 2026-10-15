use super::*;

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
