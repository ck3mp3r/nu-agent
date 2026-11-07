use super::*;

#[tokio::test]
async fn concurrent_in_flight_rules_allow_multiple_mcp_toggles() {
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
            UiRequest::ToggleMcp {
                server: "gh".to_string(),
                enable: false,
            },
            &mut ctx,
        )
        .await;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::ToggleMcp {
                server: "fs".to_string(),
                enable: true,
            },
            &mut ctx,
        )
        .await;

    let mut mcp_toggles = 0;
    while let Some(cmd) = recv_command(worker_rx) {
        if matches!(
            cmd,
            WorkerCommand::HandleUiRequest {
                request: UiRequest::ToggleMcp { .. },
                ..
            }
        ) {
            mcp_toggles += 1;
        }
    }
    assert_eq!(
        mcp_toggles, 2,
        "multiple MCP toggles may be in-flight simultaneously"
    );

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_concurrent_response(
            UiRequestResponse::McpToggle {
                server: "gh".to_string(),
                result: Ok(McpUsabilityState::Disabled),
                total: 0,
                server_count: 0,
                names_by_server: vec![],
            },
            &mut ctx,
        )
        .await;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_concurrent_response(
            UiRequestResponse::McpToggle {
                server: "fs".to_string(),
                result: Ok(McpUsabilityState::Enabled),
                total: 0,
                server_count: 0,
                names_by_server: vec![],
            },
            &mut ctx,
        )
        .await;

    assert!(!stage.has_pending());
}

#[tokio::test]
async fn session_switch_success_rehydrates_transcript_from_snapshots() {
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
                result: Ok(vec![
                    UiMessageSnapshot::new("user", "first"),
                    UiMessageSnapshot::new("assistant", "second"),
                ]),
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
            matches!(e, UiStateEvent::HydrateTranscript { messages, .. } if messages.len() == 2)
        }),
        "transcript must be re-hydrated from all snapshots"
    );
}
