use super::*;

// ---------------------------------------------------------------------------
// Replaced deferred prompt rejects its A2A task
// ---------------------------------------------------------------------------

/// A deferred prompt that a later prompt replaces must reject the A2A task that
/// queued it, so the caller reaches a terminal state instead of waiting for the
/// send timeout.
#[tokio::test]
async fn a_replaced_deferred_prompt_rejects_its_a2a_task() -> Result<()> {
    // -- Setup & Fixtures
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
    // The server handler transitions an incoming task Submitted→Working before
    // the orchestrator sees it. Model that here so the rejection has a Working
    // task to transition.
    let store = std::sync::Arc::new(InMemoryTaskStore::default());
    let task1 = store.create_task(None, None, None);
    store
        .update_status(&task1.id, TaskState::Working, None)
        .map_err(|e| format!("task-1 should reach Working: {e}"))?;
    let task1_id = task1.id.clone();
    state.task_store = Some(std::sync::Arc::clone(&store));

    let trace = Trace::default();
    let mut slash = MockSlash::with_trace(std::sync::Arc::clone(&trace));
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::with_trace(std::sync::Arc::clone(&trace));
    ui_request.blocking_pending = true;
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    // -- Exec
    bus.external()
        .send(ExternalEvent::PromptReceived {
            prompt: "external task".to_string(),
            task_id: task1_id.clone(),
            context_id: None,
        })
        .await
        .map_err(|_| "the external channel should be open")?;
    // The external prompt defers, then a second prompt replaces it.
    for text in ["external task", "replacement"] {
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
    // Quit only after the blocking response is handled: the loop must not exit
    // before it can submit the replacement prompt.
    let quit_tx = event_tx.clone();
    let gate = std::sync::Arc::clone(&trace);
    tokio::spawn(async move {
        while !gate
            .lock()
            .unwrap()
            .iter()
            .any(|entry| entry == "blocking_response")
        {
            tokio::task::yield_now().await;
        }
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
    wait_for_task_state(&store, &task1_id, TaskState::Rejected).await?;
    let rejected = store
        .get_task(&task1_id)
        .map_err(|e| format!("task-1 should exist: {e}"))?;
    let message = rejected
        .status
        .message
        .ok_or("the rejected task should carry a status message")?;
    let text = match message.parts.first() {
        Some(Part::Text { text }) => text.clone(),
        _ => return Err("the status message should carry a text part".into()),
    };
    assert_eq!(
        text,
        crate::orchestrator::handlers::DEFERRED_PROMPT_SUPERSEDED_REASON,
        "the rejected task must carry the supersession reason"
    );
    assert_eq!(
        slash.handle_calls,
        vec!["replacement".to_string()],
        "the replacement prompt must still be submitted"
    );
    Ok(())
}

/// A replaced deferred prompt with no task store logs and continues: the loop
/// must not panic when it cannot reject the superseded task.
#[tokio::test]
async fn a_replaced_deferred_prompt_without_store_logs_and_continues() -> Result<()> {
    // -- Setup & Fixtures
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
    // No task store: the supersession branch must log and continue.
    state.task_store = None;

    let trace = Trace::default();
    let mut slash = MockSlash::with_trace(std::sync::Arc::clone(&trace));
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::with_trace(std::sync::Arc::clone(&trace));
    ui_request.blocking_pending = true;
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    // -- Exec
    bus.external()
        .send(ExternalEvent::PromptReceived {
            prompt: "external task".to_string(),
            task_id: "task-1".to_string(),
            context_id: None,
        })
        .await
        .map_err(|_| "the external channel should be open")?;
    for text in ["external task", "replacement"] {
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
    let quit_tx = event_tx.clone();
    let gate = std::sync::Arc::clone(&trace);
    tokio::spawn(async move {
        while !gate
            .lock()
            .unwrap()
            .iter()
            .any(|entry| entry == "blocking_response")
        {
            tokio::task::yield_now().await;
        }
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
    assert_eq!(
        slash.handle_calls,
        vec!["replacement".to_string()],
        "the replacement prompt must still be submitted without a store"
    );
    Ok(())
}
