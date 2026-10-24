use super::*;

// ---------------------------------------------------------------------------
// Deferred prompt submission
// ---------------------------------------------------------------------------

/// A prompt deferred while a blocking request was pending is submitted once the
/// blocking request completes — never discarded.
#[tokio::test]
async fn deferred_prompt_submits_when_the_blocking_request_completes() -> Result<()> {
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
    let trace = Trace::default();
    let mut slash = MockSlash::with_trace(std::sync::Arc::clone(&trace));
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::with_trace(std::sync::Arc::clone(&trace));
    ui_request.blocking_pending = true;
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    // -- Exec
    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: "hello".to_string(),
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
    // Quit only after the blocking response is handled: the loop must not exit
    // before it can submit the deferred prompt.
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
        vec!["hello".to_string()],
        "the deferred prompt must be submitted, not discarded"
    );
    assert_eq!(
        *trace.lock().unwrap(),
        vec!["blocking_response".to_string(), "slash:hello".to_string()],
        "the prompt must be submitted only after the blocking request completes"
    );
    Ok(())
}

/// Only one prompt defers at a time: a second prompt that arrives while one is
/// deferred replaces the first.
#[tokio::test]
async fn a_second_deferred_prompt_replaces_the_first() -> Result<()> {
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
    let trace = Trace::default();
    let mut slash = MockSlash::with_trace(std::sync::Arc::clone(&trace));
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::with_trace(std::sync::Arc::clone(&trace));
    ui_request.blocking_pending = true;
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    // -- Exec
    for text in ["first", "second"] {
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
        vec!["second".to_string()],
        "the second prompt replaces the first deferred prompt"
    );
    Ok(())
}

/// A deferred prompt is not submitted once a quit is pending: the loop is
/// tearing down and must not start a new turn.
#[tokio::test]
async fn deferred_prompt_is_not_submitted_when_quit_is_pending() -> Result<()> {
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
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    ui_request.blocking_pending = true;
    ui_request.pending = true;
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    // -- Exec
    event_tx
        .send(OrchestratorEvent::Quit)
        .await
        .map_err(|_| "the event channel should be open")?;
    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: "hello".to_string(),
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
    assert!(
        slash.handle_calls.is_empty(),
        "a quit-pending loop must not start a new turn"
    );
    Ok(())
}
