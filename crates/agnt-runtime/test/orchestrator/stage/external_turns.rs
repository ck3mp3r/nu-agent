use super::*;

// ---------------------------------------------------------------------------
// Session stage — external task outcome publication
// ---------------------------------------------------------------------------

fn take_turn_events(turn_rx: &mut crate::bus::TurnRx) -> Vec<TurnEvent> {
    let mut out = Vec::new();
    while let Ok(event) = turn_rx.try_recv() {
        out.push(event);
    }
    out
}

#[tokio::test]
async fn external_turn_error_publishes_task_failed() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = Harness::new();
    let HarnessParts {
        stage: _,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx: _,
        ui_event_rx: _,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut turn_rx = bus.turn().subscribe();
    state.active_external_prompt = Some("external task".to_string());
    state.active_external_task_id = Some("task-1".to_string());
    let mut session = SessionStage;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    // -- Exec
    session
        .handle_outcome(
            TurnOutcome::Error(LabeledError::new("provider exploded")),
            &mut ctx,
        )
        .await;

    // -- Check
    let events = take_turn_events(&mut turn_rx);
    assert_eq!(
        events.len(),
        1,
        "exactly one turn event should be published, got {events:?}"
    );
    match &events[0] {
        TurnEvent::TaskFailed { task_id, error } => {
            assert_eq!(task_id, "task-1");
            assert_eq!(error, "provider exploded");
        }
        other => return Err(format!("expected TaskFailed, got {other:?}").into()),
    }
    assert_eq!(state.active_external_task_id, None);
    assert_eq!(state.active_external_prompt, None);
    Ok(())
}

#[tokio::test]
async fn external_turn_cancelled_publishes_task_failed() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = Harness::new();
    let HarnessParts {
        stage: _,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx: _,
        ui_event_rx: _,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut turn_rx = bus.turn().subscribe();
    state.active_external_prompt = Some("external task".to_string());
    state.active_external_task_id = Some("task-2".to_string());
    let mut session = SessionStage;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    // -- Exec
    session
        .handle_outcome(TurnOutcome::Cancelled, &mut ctx)
        .await;

    // -- Check
    let events = take_turn_events(&mut turn_rx);
    assert_eq!(
        events.len(),
        1,
        "exactly one turn event should be published, got {events:?}"
    );
    match &events[0] {
        TurnEvent::TaskFailed { task_id, error } => {
            assert_eq!(task_id, "task-2");
            assert_eq!(
                error,
                crate::orchestrator::stages::session::TASK_CANCELLED_NOTICE
            );
        }
        other => return Err(format!("expected TaskFailed, got {other:?}").into()),
    }
    assert_eq!(state.active_external_task_id, None);
    Ok(())
}

#[tokio::test]
async fn external_turn_success_publishes_task_completed() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = Harness::new();
    let HarnessParts {
        stage: _,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx: _,
        ui_event_rx: _,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut turn_rx = bus.turn().subscribe();
    state.active_external_prompt = Some("external task".to_string());
    state.active_external_task_id = Some("task-3".to_string());
    let mut session = SessionStage;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    // -- Exec
    session
        .handle_outcome(
            TurnOutcome::Success(Value::string("done", Span::test_data())),
            &mut ctx,
        )
        .await;

    // -- Check
    let events = take_turn_events(&mut turn_rx);
    assert_eq!(
        events.len(),
        1,
        "exactly one turn event should be published, got {events:?}"
    );
    match &events[0] {
        TurnEvent::TaskCompleted { task_id, .. } => assert_eq!(task_id, "task-3"),
        other => return Err(format!("expected TaskCompleted, got {other:?}").into()),
    }
    Ok(())
}

#[tokio::test]
async fn non_external_turn_error_publishes_no_task_failed() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = Harness::new();
    let HarnessParts {
        stage: _,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx: _,
        ui_event_rx: _,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut turn_rx = bus.turn().subscribe();
    let mut session = SessionStage;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    // -- Exec
    session
        .handle_outcome(TurnOutcome::Error(LabeledError::new("boom")), &mut ctx)
        .await;

    // -- Check
    let events = take_turn_events(&mut turn_rx);
    assert!(
        events.is_empty(),
        "a user turn failure must not publish a task event, got {events:?}"
    );
    Ok(())
}
