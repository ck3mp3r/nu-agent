use super::*;

// ---------------------------------------------------------------------------
// External prompt and fatal error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn external_prompt_dispatches_turn_when_idle() -> Result<()> {
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
    let mut slash = SlashStage;
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

    // The external prompt is enqueued on the control concern: the loop is its
    // single intake consumer and submits it itself, carrying the task id.
    bus.control()
        .publish(crate::bus::ControlCommand::EnqueueExternalPrompt {
            task_id: "task-1".to_string(),
            prompt: "external task".to_string(),
        })
        .map_err(|e| format!("{e:?}"))?;

    // No cancel source is configured, so the surface closing its intake is the
    // loop's exit; the attributed turn is the last thing it does.
    let driver = async {
        match recv_worker_command(&mut worker_rx, "the attributed turn").await? {
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
            _ => return Err("expected ExecuteTurn".into()),
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
        ctx_state.worker_active,
        "worker_active should be true after the turn is dispatched"
    );
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "the intake dispatches the turn once"
    );
    Ok(())
}

#[tokio::test]
async fn fatal_error_returns_err() -> Result<()> {
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

    event_tx
        .send(OrchestratorEvent::FatalError(LabeledError::new("fatal")))
        .await
        .map_err(|e| format!("{e:?}"))?;

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

    let Err(err) = result else {
        return Err("run_orchestrator_loop should fail with fatal error".into());
    };
    assert_eq!(err.msg, "fatal");
    Ok(())
}
