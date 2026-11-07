use super::*;

#[tokio::test]
async fn switch_model_queue_last_write_wins() -> Result<()> {
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
                spec: "first-model".to_string(),
            },
            &mut ctx,
        )
        .await;
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "second-model".to_string(),
            },
            &mut ctx,
        )
        .await;

    assert!(recv_command(p.worker_rx).is_none());
    assert!(!p.stage.has_blocking_pending());

    p.ctx_state.worker_active = false;
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.drain_queued(&mut ctx).await;

    let cmd = recv_command(p.worker_rx).ok_or("queued command should be dispatched")?;
    match cmd {
        WorkerCommand::HandleUiRequest { request, .. } => {
            assert!(
                matches!(request, UiRequest::SwitchModel { spec } if spec == "second-model"),
                "last queued model switch should win"
            );
        }
        _ => panic!("expected HandleUiRequest, got unexpected variant"),
    }
    Ok(())
}

#[tokio::test]
async fn switch_agent_queue_last_write_wins() -> Result<()> {
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
                name: "agent-a".to_string(),
            },
            &mut ctx,
        )
        .await;
    p.stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "agent-b".to_string(),
            },
            &mut ctx,
        )
        .await;

    assert!(recv_command(p.worker_rx).is_none());
    assert!(!p.stage.has_blocking_pending());

    p.ctx_state.worker_active = false;
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.drain_queued(&mut ctx).await;

    let cmd = recv_command(p.worker_rx).ok_or("queued command should be dispatched")?;
    match cmd {
        WorkerCommand::HandleUiRequest { request, .. } => {
            assert!(
                matches!(request, UiRequest::SwitchAgent { name } if name == "agent-b"),
                "last queued agent switch should win"
            );
        }
        _ => panic!("expected HandleUiRequest, got unexpected variant"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// drain_queued tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn drain_queued_dispatches_when_idle() -> Result<()> {
    let mut h = Harness::new();
    let p = h.parts();
    // Queue a model switch while worker is active.
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
    assert!(recv_command(p.worker_rx).is_none());

    // Now worker becomes idle — drain should dispatch.
    p.ctx_state.worker_active = false;
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.drain_queued(&mut ctx).await;

    let cmd = recv_command(p.worker_rx).ok_or("queued command should be dispatched")?;
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
async fn drain_queued_does_nothing_when_worker_active() {
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
    assert!(recv_command(p.worker_rx).is_none());

    // Still active — drain should not dispatch.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.drain_queued(&mut ctx).await;
    assert!(recv_command(p.worker_rx).is_none());
}

#[tokio::test]
async fn drain_queued_does_nothing_when_blocking_pending() {
    let mut h = Harness::new();
    let p = h.parts();
    // Dispatch a model switch to occupy pending_blocking.
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

    // Queue another while blocking is pending.
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
    assert!(recv_command(p.worker_rx).is_none());

    // Drain should not dispatch because blocking is still pending.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.drain_queued(&mut ctx).await;
    assert!(recv_command(p.worker_rx).is_none());
}

// ---------------------------------------------------------------------------
// has_pending tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn has_pending_reflects_blocking_and_concurrent() {
    let mut h = Harness::new();
    let p = h.parts();

    // Initially no pending.
    assert!(!p.stage.has_blocking_pending());
    assert!(!p.stage.has_pending());

    // Dispatch a blocking request.
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
    assert!(p.stage.has_blocking_pending());
    assert!(p.stage.has_pending());

    // Dispatch a concurrent request.
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
    let _ = recv_command(p.worker_rx);
    assert!(p.stage.has_pending());

    // Clear blocking.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_blocking_response(
            UiRequestResponse::ModelSwitch(Ok(("test-model".to_string(), None))),
            &mut ctx,
        )
        .await;
    assert!(!p.stage.has_blocking_pending());
    assert!(p.stage.has_pending()); // concurrent still pending

    // Clear concurrent.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_concurrent_response(
            UiRequestResponse::McpToggle {
                server: "test-server".to_string(),
                result: Ok(McpUsabilityState::Enabled),
                total: 0,
                server_count: 0,
                names_by_server: Vec::new(),
            },
            &mut ctx,
        )
        .await;
    assert!(!p.stage.has_pending());
}
