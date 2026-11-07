use super::*;

// ---------------------------------------------------------------------------
// Model switch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn model_switch_send_failure_emits_warning() {
    let mut h = Harness::new();
    worker_rx_drop(&mut h);
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
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "gpt-4o".to_string(),
            },
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(ui_event_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Model switch worker channel closed")
    );
    assert!(!stage.has_blocking_pending());
}

#[tokio::test]
async fn model_switch_error_response_emits_warning() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        ui_event_rx,
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

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_blocking_response(
            UiRequestResponse::ModelSwitch(Err("model failed".to_string())),
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(ui_event_rx);
    assert!(warnings.iter().any(|w| w == "model failed"));
    let cmd = recv_command(worker_rx).ok_or("HandleUiRequest should be dispatched")?;
    assert!(
        matches!(cmd, WorkerCommand::HandleUiRequest { .. }),
        "expected HandleUiRequest command"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Agent switch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn agent_switch_send_failure_emits_warning() {
    let mut h = Harness::new();
    worker_rx_drop(&mut h);
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
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "research".to_string(),
            },
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(ui_event_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Agent switch worker channel closed")
    );
    assert!(!stage.has_blocking_pending());
}

#[tokio::test]
async fn agent_switch_error_response_emits_warning() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        ui_event_rx,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "research".to_string(),
            },
            &mut ctx,
        )
        .await;

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_blocking_response(
            UiRequestResponse::AgentSwitch(Err("agent failed".to_string())),
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(ui_event_rx);
    assert!(warnings.iter().any(|w| w == "agent failed"));
    let cmd = recv_command(worker_rx).ok_or("HandleUiRequest should be dispatched")?;
    assert!(
        matches!(cmd, WorkerCommand::HandleUiRequest { .. }),
        "expected HandleUiRequest command"
    );
    Ok(())
}
