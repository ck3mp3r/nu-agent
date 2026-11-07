use super::*;

// ---------------------------------------------------------------------------
// Session switch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn session_switch_while_worker_active_is_rejected_and_not_queued() {
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
    state.worker_active = true;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "sess-1".to_string(),
            },
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(ui_event_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Cannot switch session while worker is active")
    );
    assert!(
        recv_command(worker_rx).is_none(),
        "no worker command should be dispatched"
    );
}

#[tokio::test]
async fn session_switch_while_pending_is_ignored() {
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
            UiRequest::SwitchSession {
                id: "sess-1".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(worker_rx);

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "sess-2".to_string(),
            },
            &mut ctx,
        )
        .await;

    assert!(
        recv_command(worker_rx).is_none(),
        "only one session switch should be dispatched"
    );
    let warnings = take_warnings(ui_event_rx);
    assert!(
        !warnings.iter().any(|w| w.contains("sess-2")),
        "no warning should mention the ignored second request, got {warnings:?}"
    );
    assert!(stage.has_blocking_pending());
}

#[tokio::test]
async fn session_switch_success_clears_and_hydrates_transcript() {
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
        session_rx,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "sess-1".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(worker_rx);

    let snapshot = UiMessageSnapshot::new("user", "hello");
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_blocking_response(
            UiRequestResponse::SessionSwitch {
                id: "sess-1".to_string(),
                result: Ok(vec![snapshot]),
            },
            &mut ctx,
        )
        .await;

    let ui_events = take_ui_state(ui_state_rx);
    assert!(
        ui_events
            .iter()
            .any(|e| matches!(e, UiStateEvent::ClearTranscript)),
        "transcript must be cleared"
    );
    assert!(
        ui_events
            .iter()
            .any(|e| matches!(e, UiStateEvent::PushStartupLogo)),
        "startup logo must be pushed before the hydration"
    );
    assert!(
        ui_events.iter().any(|e| {
            matches!(e, UiStateEvent::HydrateTranscript { messages, .. } if messages.len() == 1)
        }),
        "transcript must be re-hydrated"
    );
    let session_events = take_session_events(session_rx);
    assert!(
        session_events.iter().any(|e| {
            matches!(e, SessionEvent::Switched { from_session_id: None, to_session_id } if to_session_id == "sess-1")
        })
    );
    assert!(!stage.has_blocking_pending());
}

#[tokio::test]
async fn session_switch_error_emits_warning() {
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
            UiRequest::SwitchSession {
                id: "sess-1".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(worker_rx);

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_blocking_response(
            UiRequestResponse::SessionSwitch {
                id: "sess-1".to_string(),
                result: Err("session load failed".to_string()),
            },
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(ui_event_rx);
    assert!(warnings.iter().any(|w| w == "session load failed"));
    assert!(!stage.has_blocking_pending());
}

#[tokio::test]
async fn session_switch_send_failure_emits_warning() {
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
            UiRequest::SwitchSession {
                id: "sess-1".to_string(),
            },
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(ui_event_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Session switch worker channel closed")
    );
    assert!(!stage.has_blocking_pending());
}
