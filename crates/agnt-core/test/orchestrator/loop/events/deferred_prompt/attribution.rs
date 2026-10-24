use super::*;

// ---------------------------------------------------------------------------
// Deferred prompt task attribution
// ---------------------------------------------------------------------------

/// A deferred prompt that a later prompt replaces must not hand its A2A task id
/// to the replacement: the replacement is a user prompt, so its turn carries no
/// task attribution.
#[tokio::test]
async fn a_replaced_deferred_prompt_leaves_a_later_prompt_unattributed() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = Harness::new();
    let HarnessParts {
        event_tx,
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
    let mut slash = SlashStage;
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    ui_request.blocking_pending = true;
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    let mut turn_rx = bus.turn().subscribe();

    // -- Exec
    bus.external()
        .send(ExternalEvent::PromptReceived {
            prompt: "external task".to_string(),
            task_id: "task-1".to_string(),
            context_id: None,
        })
        .await
        .map_err(|_| "the external channel should be open")?;
    // The external prompt defers, then a user prompt replaces it.
    for text in ["external task", "user prompt"] {
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
    // Exit cleanly once the replacement prompt has started its turn.
    let mut gate_turn_rx = bus.turn().subscribe();
    let result_tx = worker_result_tx.clone();
    let quit_tx = event_tx.clone();
    tokio::spawn(async move {
        loop {
            match gate_turn_rx.recv().await {
                Ok(TurnEvent::Started { .. }) => break,
                Ok(_) => continue,
                Err(_) => return,
            }
        }
        let _ = result_tx
            .send(TurnOutcome::Success(Value::nothing(Span::test_data())))
            .await;
        let _ = quit_tx.send(OrchestratorEvent::Quit).await;
    });

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        run_orchestrator_loop(
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
        ),
    )
    .await
    .map_err(|_| "the orchestrator loop should exit")?;

    // -- Check
    assert!(result.is_ok());
    assert_unattributed_turn(&mut turn_rx, "the replacement prompt").await?;
    assert_eq!(
        state.active_external_prompt, None,
        "the replacement prompt must not be recorded as an external prompt"
    );
    assert_eq!(
        state.active_external_task_id, None,
        "the replacement prompt must not be attributed to the replaced task"
    );
    assert_eq!(
        state.pending_a2a_task_id, None,
        "the replaced task id must not linger in the pending slot"
    );
    Ok(())
}

/// A deferred external prompt keeps its A2A task attribution when it is
/// submitted after the blocking request completes.
#[tokio::test]
async fn a_deferred_external_prompt_keeps_its_task_attribution() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = Harness::new();
    let HarnessParts {
        event_tx,
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
    let mut slash = SlashStage;
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    ui_request.blocking_pending = true;
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    let mut turn_rx = bus.turn().subscribe();

    // -- Exec
    bus.external()
        .send(ExternalEvent::PromptReceived {
            prompt: "external task".to_string(),
            task_id: "task-1".to_string(),
            context_id: None,
        })
        .await
        .map_err(|_| "the external channel should be open")?;
    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: "external task".to_string(),
        })
        .await
        .map_err(|_| "the event channel should be open")?;
    blocking_tx
        .send(UiRequestResponse::ModelSwitch(Ok((
            "test-model".to_string(),
            None,
        ))))
        .await
        .map_err(|_| "the blocking response channel should be open")?;
    // Exit cleanly once the deferred prompt has started its turn.
    let mut gate_turn_rx = bus.turn().subscribe();
    let result_tx = worker_result_tx.clone();
    let quit_tx = event_tx.clone();
    tokio::spawn(async move {
        loop {
            match gate_turn_rx.recv().await {
                Ok(TurnEvent::Started { .. }) => break,
                Ok(_) => continue,
                Err(_) => return,
            }
        }
        let _ = result_tx
            .send(TurnOutcome::Success(Value::nothing(Span::test_data())))
            .await;
        let _ = quit_tx.send(OrchestratorEvent::Quit).await;
    });

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        run_orchestrator_loop(
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
        ),
    )
    .await
    .map_err(|_| "the orchestrator loop should exit")?;

    // -- Check
    assert!(result.is_ok());
    let started = turn_rx
        .try_recv()
        .map_err(|e| format!("TurnEvent::Started should be published: {e:?}"))?;
    match started {
        TurnEvent::Started { task_id, .. } => assert_eq!(
            task_id.as_deref(),
            Some("task-1"),
            "the deferred external prompt must keep its task attribution"
        ),
        _ => return Err("expected TurnEvent::Started".into()),
    }
    assert_eq!(
        state.active_external_task_id.as_deref(),
        Some("task-1"),
        "the deferred external prompt must stay the active external task"
    );
    Ok(())
}
