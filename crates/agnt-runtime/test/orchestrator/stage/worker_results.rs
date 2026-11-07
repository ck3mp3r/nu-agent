use super::*;

// ---------------------------------------------------------------------------
// Cross-cutting
// ---------------------------------------------------------------------------

#[tokio::test]
async fn worker_result_processed_before_model_switch_result() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx: _,
        ui_event_rx,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut session = SessionStage;
    state.worker_active = true;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    session
        .handle_outcome(
            TurnOutcome::Error(LabeledError::new("turn failed")),
            &mut ctx,
        )
        .await;

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_blocking_response(
            UiRequestResponse::ModelSwitch(Err("model failed".to_string())),
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(ui_event_rx);
    let session_idx = warnings
        .iter()
        .position(|w| w == "turn failed")
        .ok_or("session turn error warning should be present")?;
    let model_idx = warnings
        .iter()
        .position(|w| w == "model failed")
        .ok_or("model switch error warning should be present")?;
    assert!(
        session_idx < model_idx,
        "session result must be processed before model result, got {warnings:?}"
    );
    Ok(())
}

/// The stage must pass the LabeledError message through verbatim — it adds no
/// "Turn failed: " prefix (the executor owns the prefix).
#[tokio::test]
async fn turn_error_warning_passes_message_verbatim() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = Harness::new();
    let HarnessParts {
        stage: _,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx: _,
        ui_event_rx,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut session = SessionStage;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    // -- Exec
    session
        .handle_outcome(
            TurnOutcome::Error(LabeledError::new("Turn failed: output budget exhausted")),
            &mut ctx,
        )
        .await;

    // -- Check
    let warnings = take_warnings(ui_event_rx);
    assert_eq!(
        warnings,
        vec!["Turn failed: output budget exhausted".to_string()],
        "stage must pass the LabeledError message verbatim, no prefix added"
    );
    Ok(())
}

#[tokio::test]
async fn pending_model_switch_skips_slash_stage() {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        ui_event_rx: _,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "gpt-4o".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(worker_rx);

    assert!(stage.has_blocking_pending());
    assert!(
        recv_command(worker_rx).is_none(),
        "no ExecuteTurn command should be dispatched"
    );
}
