use super::*;

// ---------------------------------------------------------------------------
// handle_concurrent_response tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_response_mcp_toggle_success() {
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
            UiRequest::ToggleMcp {
                server: "test-server".to_string(),
                enable: true,
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

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
                total: 5,
                server_count: 3,
                names_by_server: vec![("test-server".to_string(), vec!["tool1".to_string()])],
            },
            &mut ctx,
        )
        .await;

    let ui_events = take_ui_state(p.ui_state_rx);
    assert!(ui_events.iter().any(|e| matches!(e, UiStateEvent::SetMcpServerState { server, state, error, total } if server == "test-server" && *state == McpUsabilityState::Enabled && error.is_none() && *total == 5)));
    assert!(ui_events.iter().any(|e| matches!(e, UiStateEvent::SetMcpVisibleToolCount { server, count } if server == "test-server" && *count == 3)));
    assert!(ui_events.iter().any(|e| matches!(e, UiStateEvent::SetMcpVisibleToolNames { server, names } if server == "test-server" && names == &vec!["tool1".to_string()])));
    assert!(!p.stage.has_pending());
}

#[tokio::test]
async fn concurrent_response_mcp_toggle_error() {
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
            UiRequest::ToggleMcp {
                server: "test-server".to_string(),
                enable: true,
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

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
                result: Err("toggle failed".to_string()),
                total: 5,
                server_count: 3,
                names_by_server: Vec::new(),
            },
            &mut ctx,
        )
        .await;

    let ui_events = take_ui_state(p.ui_state_rx);
    assert!(ui_events.iter().any(|e| matches!(e, UiStateEvent::SetMcpServerState { server, state, error, total } if server == "test-server" && *state == McpUsabilityState::Failed && error.as_deref() == Some("toggle failed") && *total == 5)));
    assert!(!p.stage.has_pending());
}

#[tokio::test]
async fn concurrent_response_session_refresh_success() {
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

    let session = SessionInfo {
        id: "session-1".to_string(),
        message_count: 3,
        last_active: chrono::Utc::now(),
        title: Some("Test".to_string()),
    };
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_concurrent_response(
            UiRequestResponse::SessionRefresh(Ok(vec![session])),
            &mut ctx,
        )
        .await;

    let ui_events = take_ui_state(p.ui_state_rx);
    assert!(ui_events.iter().any(
        |e| matches!(e, UiStateEvent::SetSessionPickerOptions(sessions) if sessions.len() == 1)
    ));
    assert!(!p.stage.has_pending());
}

#[tokio::test]
async fn concurrent_response_session_refresh_error() {
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

    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_concurrent_response(
            UiRequestResponse::SessionRefresh(Err("refresh failed".to_string())),
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(p.ui_event_rx);
    assert!(warnings.iter().any(|w| w == "refresh failed"));
    assert!(!p.stage.has_pending());
}
