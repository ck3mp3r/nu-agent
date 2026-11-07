use super::*;

// ---------------------------------------------------------------------------
// handle_incoming tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn switch_model_dispatches_when_idle() -> Result<()> {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "test-model".to_string(),
            },
            &mut ctx,
        )
        .await;

    let cmd = recv_command(p.worker_rx).ok_or("command should be dispatched")?;
    match cmd {
        WorkerCommand::HandleUiRequest { request, .. } => {
            assert!(matches!(request, UiRequest::SwitchModel { spec } if spec == "test-model"));
        }
        _ => panic!("expected HandleUiRequest, got unexpected variant"),
    }
    assert!(p.stage.has_blocking_pending());
    Ok(())
}

#[tokio::test]
async fn switch_model_queues_when_worker_active() {
    let mut h = Harness::new();
    let p = h.parts();
    p.ctx_state.worker_active = true;
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "test-model".to_string(),
            },
            &mut ctx,
        )
        .await;

    assert!(
        recv_command(p.worker_rx).is_none(),
        "no command should be dispatched"
    );
    assert!(!p.stage.has_blocking_pending());
}

#[tokio::test]
async fn switch_agent_dispatches_when_idle() -> Result<()> {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "research-agent".to_string(),
            },
            &mut ctx,
        )
        .await;

    let cmd = recv_command(p.worker_rx).ok_or("command should be dispatched")?;
    match cmd {
        WorkerCommand::HandleUiRequest { request, .. } => {
            assert!(matches!(request, UiRequest::SwitchAgent { name } if name == "research-agent"));
        }
        _ => panic!("expected HandleUiRequest, got unexpected variant"),
    }
    assert!(p.stage.has_blocking_pending());
    Ok(())
}

#[tokio::test]
async fn switch_agent_queues_when_worker_active() {
    let mut h = Harness::new();
    let p = h.parts();
    p.ctx_state.worker_active = true;
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "research-agent".to_string(),
            },
            &mut ctx,
        )
        .await;

    assert!(
        recv_command(p.worker_rx).is_none(),
        "no command should be dispatched"
    );
}

#[tokio::test]
async fn switch_session_dispatches_when_idle() -> Result<()> {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "session-1".to_string(),
            },
            &mut ctx,
        )
        .await;

    let cmd = recv_command(p.worker_rx).ok_or("command should be dispatched")?;
    match cmd {
        WorkerCommand::HandleUiRequest { request, .. } => {
            assert!(matches!(request, UiRequest::SwitchSession { id } if id == "session-1"));
        }
        _ => panic!("expected HandleUiRequest, got unexpected variant"),
    }
    assert!(p.stage.has_blocking_pending());
    Ok(())
}

#[tokio::test]
async fn switch_session_rejects_when_worker_active() {
    let mut h = Harness::new();
    let p = h.parts();
    p.ctx_state.worker_active = true;
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "session-1".to_string(),
            },
            &mut ctx,
        )
        .await;

    assert!(
        recv_command(p.worker_rx).is_none(),
        "no command should be dispatched"
    );
    let warnings = take_warnings(p.ui_event_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Cannot switch session while worker is active")
    );
}

#[tokio::test]
async fn switch_session_ignored_when_blocking_pending() {
    let mut h = Harness::new();
    let p = h.parts();
    // First dispatch a model switch to occupy pending_blocking.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "test-model".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

    // Now try a session switch — should be ignored silently.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "session-1".to_string(),
            },
            &mut ctx,
        )
        .await;

    assert!(
        recv_command(p.worker_rx).is_none(),
        "no command should be dispatched"
    );
    assert!(
        take_warnings(p.ui_event_rx).is_empty(),
        "no warnings should be emitted"
    );
}

#[tokio::test]
async fn toggle_mcp_dispatches_immediately() -> Result<()> {
    let mut h = Harness::new();
    let p = h.parts();
    p.ctx_state.worker_active = true; // MCP toggle works even when worker is active
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::ToggleMcp {
                server: "test-server".to_string(),
                enable: true,
            },
            &mut ctx,
        )
        .await;

    let cmd = recv_command(p.worker_rx).ok_or("command should be dispatched")?;
    match cmd {
        WorkerCommand::HandleUiRequest { request, .. } => {
            assert!(
                matches!(request, UiRequest::ToggleMcp { server, enable } if server == "test-server" && enable)
            );
        }
        _ => panic!("expected HandleUiRequest, got unexpected variant"),
    }
    assert!(p.stage.has_pending());
    Ok(())
}

#[tokio::test]
async fn refresh_session_picker_dispatches_when_not_in_flight() -> Result<()> {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;

    let cmd = recv_command(p.worker_rx).ok_or("command should be dispatched")?;
    match cmd {
        WorkerCommand::HandleUiRequest { request, .. } => {
            assert!(matches!(request, UiRequest::RefreshSessionPicker));
        }
        _ => panic!("expected HandleUiRequest, got unexpected variant"),
    }
    Ok(())
}

#[tokio::test]
async fn refresh_session_picker_skips_when_in_flight() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;
    let _ = recv_command(p.worker_rx);

    // Second refresh should be skipped.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;
    assert!(
        recv_command(p.worker_rx).is_none(),
        "no second command should be dispatched"
    );
}
