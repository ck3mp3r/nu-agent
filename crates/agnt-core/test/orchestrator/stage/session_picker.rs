use super::*;

// ---------------------------------------------------------------------------
// Session picker refresh
// ---------------------------------------------------------------------------

#[tokio::test]
async fn session_picker_launch_dispatches_refresh_command() -> Result<()> {
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
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;

    let cmd = recv_command(worker_rx).ok_or("RefreshSessionPicker should be dispatched")?;
    assert!(
        matches!(
            cmd,
            WorkerCommand::HandleUiRequest {
                request: UiRequest::RefreshSessionPicker,
                ..
            }
        ),
        "expected RefreshSessionPicker command"
    );
    Ok(())
}

#[tokio::test]
async fn session_picker_refresh_dispatched_when_no_pending() -> Result<()> {
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
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;

    let cmd = recv_command(worker_rx).ok_or("RefreshSessionPicker should be dispatched")?;
    assert!(
        matches!(
            cmd,
            WorkerCommand::HandleUiRequest {
                request: UiRequest::RefreshSessionPicker,
                ..
            }
        ),
        "expected RefreshSessionPicker command when no refresh is pending"
    );
    Ok(())
}

#[tokio::test]
async fn session_picker_refresh_in_flight_skips_new_command() {
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
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;
    let _ = recv_command(worker_rx);

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;

    assert!(
        recv_command(worker_rx).is_none(),
        "only one RefreshSessionPicker should be dispatched while one is pending"
    );
    assert!(stage.has_pending());
}

#[tokio::test]
async fn session_picker_refresh_success_sets_options() {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        ui_event_rx: _,
        ui_state_rx,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;
    let _ = recv_command(worker_rx);

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_concurrent_response(
            UiRequestResponse::SessionRefresh(Ok(vec![session_info("sess-1")])),
            &mut ctx,
        )
        .await;

    let ui_events = take_ui_state(ui_state_rx);
    assert!(
        ui_events.iter().any(|e| {
            matches!(e, UiStateEvent::SetSessionPickerOptions(sessions) if sessions.len() == 1)
        }),
        "options must be set"
    );
    assert!(
        ui_events.iter().any(|e| {
            matches!(e, UiStateEvent::SetSessionPickerOptions(sessions) if sessions.len() == 1 && sessions[0].id == "sess-1")
        }),
        "options[0] must be sess-1"
    );
    assert!(!stage.has_pending());
}

#[tokio::test]
async fn session_picker_refresh_error_warns() {
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
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;
    let _ = recv_command(worker_rx);

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_concurrent_response(
            UiRequestResponse::SessionRefresh(Err("refresh failed".to_string())),
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(ui_event_rx);
    assert!(warnings.iter().any(|w| w == "refresh failed"));
    assert!(!stage.has_pending());
}

#[tokio::test]
async fn session_picker_refresh_send_failure_warns() {
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
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;

    let warnings = take_warnings(ui_event_rx);
    assert!(warnings.iter().any(|w| w == "Worker channel closed"));
    assert!(!stage.has_pending());
}
