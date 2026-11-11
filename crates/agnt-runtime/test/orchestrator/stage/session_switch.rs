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
        narration_rx,
        lifecycle_rx: _,
        stream_rx: _,
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

    let warnings = take_warnings(narration_rx);
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
        narration_rx,
        lifecycle_rx: _,
        stream_rx: _,
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
    let warnings = take_warnings(narration_rx);
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
        narration_rx: _,
        lifecycle_rx,
        stream_rx,
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

    // The switch marker and the replayed rows ride the stream: the surface
    // replaces its transcript on the hydrated marker, so there is no separate
    // clear or logo event to observe.
    let records = take_records(stream_rx);
    assert!(
        records
            .iter()
            .any(|r| matches!(r, SessionRecord::Session(SessionLifecycle::Switched { .. }))),
        "the switch must re-hydrate the transcript, got {records:?}"
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| matches!(r, SessionRecord::Prompt(_)))
            .count(),
        1,
        "the replay carries one prompt row, got {records:?}"
    );
    let lifecycle = take_lifecycle(lifecycle_rx);
    assert!(
        lifecycle.iter().any(|e| matches!(
            e,
            LifecycleEvent::SessionSwitched { from: None, to } if to == "sess-1"
        )),
        "the switch must announce on the spine, got {lifecycle:?}"
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
        narration_rx,
        lifecycle_rx: _,
        stream_rx: _,
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

    let warnings = take_warnings(narration_rx);
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
        narration_rx,
        lifecycle_rx: _,
        stream_rx: _,
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

    let warnings = take_warnings(narration_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Session switch worker channel closed")
    );
    assert!(!stage.has_blocking_pending());
}
