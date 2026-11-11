use super::*;

// ---------------------------------------------------------------------------
// Deferred prompt task attribution
// ---------------------------------------------------------------------------

/// WHEN a peer's prompt waits in the loop's queue and a surface prompt is
/// deferred behind a blocking request, THE surface prompt's turn SHALL carry no
/// attribution, and the held peer prompt SHALL not run while that turn is in
/// flight.
#[tokio::test]
async fn a_deferred_surface_prompt_stays_unattributed() -> Result<()> {
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
    let mut slash = SlashStage;
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    ui_request.blocking_pending = true;
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
        .publish(crate::bus::ControlCommand::EnqueueExternalPrompt {
            task_id: "task-1".to_string(),
            prompt: "external task".to_string(),
        })
        .map_err(|_| "the control channel should be open")?;
    // The peer's prompt is held by the loop's own queue, and the surface prompt
    // is deferred behind the blocking request.
    for text in ["user prompt"] {
        event_tx
            .send(OrchestratorEvent::PromptSubmitted {
                text: text.to_string(),
            })
            .await
            .map_err(|_| "the event channel should be open")?;
    }
    blocking_tx
        .send(UiRequestResponse::ModelSwitch(Ok((
            "test-model".to_string(),
            None,
        ))))
        .await
        .map_err(|_| "the blocking response channel should be open")?;
    // Exit cleanly once the replacement prompt has started its turn: no cancel
    // source is configured, so the surface closing its intake is the loop's
    // exit.
    let driver = async {
        match recv_worker_command(&mut worker_rx, "the surface prompt's turn").await? {
            WorkerCommand::ExecuteTurn { task_id, .. } => {
                drop(event_tx);
                Ok::<_, Box<dyn std::error::Error>>(task_id)
            }
            _ => Err("expected ExecuteTurn".into()),
        }
    };
    let (result, task_id) = tokio::join!(
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
    let task_id = task_id?;

    // -- Check
    assert!(result.is_ok());
    assert_eq!(
        task_id, None,
        "a surface prompt's turn carries no attribution"
    );
    assert!(
        recv_execute_turn_task_id(&mut worker_rx, "the held peer prompt").is_err(),
        "the held peer prompt must not run while the surface turn is in flight"
    );
    Ok(())
}

/// WHEN a peer's prompt is held behind a blocking request, THE loop SHALL
/// dispatch its turn carrying that task's id once the request completes.
#[tokio::test]
async fn a_held_external_prompt_keeps_its_task_attribution() -> Result<()> {
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
    let mut slash = SlashStage;
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    ui_request.blocking_pending = true;
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
        .publish(crate::bus::ControlCommand::EnqueueExternalPrompt {
            task_id: "task-1".to_string(),
            prompt: "external task".to_string(),
        })
        .map_err(|_| "the control channel should be open")?;
    // The control command is the single intake: the loop submits the prompt
    // itself, carrying the task id it accepted.
    blocking_tx
        .send(UiRequestResponse::ModelSwitch(Ok((
            "test-model".to_string(),
            None,
        ))))
        .await
        .map_err(|_| "the blocking response channel should be open")?;
    // Exit cleanly once the deferred prompt has started its turn: no cancel
    // source is configured, so the surface closing its intake is the loop's
    // exit.
    let driver = async {
        match recv_worker_command(&mut worker_rx, "the held prompt's turn").await? {
            WorkerCommand::ExecuteTurn { task_id, .. } => {
                drop(event_tx);
                Ok::<_, Box<dyn std::error::Error>>(task_id)
            }
            _ => Err("expected ExecuteTurn".into()),
        }
    };
    let (result, task_id) = tokio::join!(
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
    let task_id = task_id?;

    // -- Check
    assert!(result.is_ok());
    assert_eq!(
        task_id.as_deref(),
        Some("task-1"),
        "the held external prompt must keep its task attribution"
    );
    Ok(())
}
