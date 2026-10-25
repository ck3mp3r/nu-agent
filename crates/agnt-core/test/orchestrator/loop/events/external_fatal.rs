use super::*;

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
        WorkerCommand::ExecuteTurn {
            prompt, task_id, ..
        } => {
            assert_eq!(prompt, "external task");
            assert_eq!(
                task_id.as_deref(),
                Some("task-1"),
                "the A2A turn must carry its task attribution"
            );
        }
        _ => panic!("expected ExecuteTurn"),
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
